use super::{
    code_generator::RandomLinkCodeGenerator,
    ports::{LinkCodeGenerator, LinkCodeResult, LinkCodes},
    queries,
    types::DiscordUserId,
};
use crate::app::AppError;
use serenity::async_trait;
use sqlx::{PgConnection, PgPool};

const MAX_CODE_ALLOCATION_ATTEMPTS: usize = 16;

#[derive(Clone)]
pub struct DatabaseLinkCodes<G = RandomLinkCodeGenerator> {
    pool: PgPool,
    generator: G,
}

impl DatabaseLinkCodes {
    pub fn new(pool: PgPool) -> Self {
        Self::with_generator(pool, RandomLinkCodeGenerator)
    }
}

impl<G: LinkCodeGenerator> DatabaseLinkCodes<G> {
    pub fn with_generator(pool: PgPool, generator: G) -> Self {
        Self { pool, generator }
    }

    async fn issue_in_transaction(
        &self,
        connection: &mut PgConnection,
        user_id: DiscordUserId,
        username: &str,
    ) -> Result<LinkCodeResult, AppError> {
        let account = queries::upsert_discord_account(connection, user_id, username).await?;
        if queries::is_discord_blocked(connection, account).await? {
            return Ok(LinkCodeResult::Blocked);
        }
        if let Some(code) = queries::unused_code(connection, account).await? {
            return Ok(LinkCodeResult::Code(code));
        }
        for _ in 0..MAX_CODE_ALLOCATION_ATTEMPTS {
            let code = self.generator.generate();
            if queries::reserve_code(connection, account, &code).await? {
                return Ok(LinkCodeResult::Code(code));
            }
        }
        Err(anyhow::anyhow!("Could not allocate a unique link code"))
    }
}

#[async_trait]
impl<G: LinkCodeGenerator> LinkCodes for DatabaseLinkCodes<G> {
    async fn issue(&self, user_id: DiscordUserId, username: &str) -> Result<LinkCodeResult, AppError> {
        let mut tx = self.pool.begin().await?;
        let result = self.issue_in_transaction(&mut tx, user_id, username).await?;
        tx.commit().await?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::mcguildlink::types::LinkCode;
    use std::{collections::VecDeque, sync::Mutex};
    static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

    struct FixedCodes(Mutex<VecDeque<&'static str>>);

    impl FixedCodes {
        fn new(codes: &[&'static str]) -> Self {
            Self(Mutex::new(codes.iter().copied().collect()))
        }
    }

    impl LinkCodeGenerator for FixedCodes {
        fn generate(&self) -> LinkCode {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected code generation")
                .into()
        }
    }

    /// コード衝突時の再生成を確認する。固定生成器で衝突を起こし、両利用者の再表示結果も検証する。
    #[sqlx::test(migrations = "../../migrations")]
    async fn colliding_code_is_retried_without_changing_another_users_code(pool: PgPool) {
        let first = DatabaseLinkCodes::with_generator(pool.clone(), FixedCodes::new(&["AC234679"]));
        assert_eq!(
            first.issue(DiscordUserId::new(10), "first").await.unwrap(),
            LinkCodeResult::Code("AC234679".into())
        );
        let second = DatabaseLinkCodes::with_generator(pool, FixedCodes::new(&["AC234679", "KMNPQRTU"]));
        assert_eq!(
            second.issue(DiscordUserId::new(20), "second").await.unwrap(),
            LinkCodeResult::Code("KMNPQRTU".into())
        );
        assert_eq!(
            first.issue(DiscordUserId::new(10), "first").await.unwrap(),
            LinkCodeResult::Code("AC234679".into())
        );
        assert_eq!(
            second.issue(DiscordUserId::new(20), "second").await.unwrap(),
            LinkCodeResult::Code("KMNPQRTU".into())
        );
    }

    /// DB の INSERT 制限を確認する。未ブロック時は成功し、ブロック済みのコード発行と双方の紐付けは拒否される。
    #[sqlx::test(migrations = "../../migrations")]
    async fn database_rejects_inserts_for_blocked_accounts(pool: PgPool) {
        sqlx::raw_sql("INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (1, 'blocked'), (2, 'allowed');
            INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ('00000000-0000-0000-0000-000000000001', 'blocked'), ('00000000-0000-0000-0000-000000000002', 'allowed');
            INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (2, 'allowed');
            INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (2, 2);
            INSERT INTO mcguildlink.block_groups (root_discord_account_id) VALUES (1);
            INSERT INTO mcguildlink.blocked_discord_accounts (discord_account_id, block_group_id) VALUES (1, 1);
            INSERT INTO mcguildlink.blocked_minecraft_accounts (minecraft_account_id, block_group_id) VALUES (1, 1);")
            .execute(&pool).await.unwrap();
        for rejected in [
            "INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'blocked')",
            "INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 2)",
            "INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (2, 1)",
        ] {
            let error = sqlx::query(rejected).execute(&pool).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
        }
    }

    async fn bot_pool(pool: &PgPool) -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query!("SET ROLE platform_bot_runtime")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with((*pool.connect_options()).clone())
            .await
            .unwrap()
    }

    /// Bot の最小権限を確認する。実運用ロールで発行・再表示・履歴照合を行い、DDL や対象外操作は拒否される。
    #[sqlx::test(migrations = "../../migrations")]
    async fn bot_role_can_issue_but_cannot_change_schema_or_unrelated_data(pool: PgPool) {
        let bot = bot_pool(&pool).await;
        let service = DatabaseLinkCodes::new(bot.clone());
        platform_database::check_migrations(&bot, &MIGRATIONS, crate::REQUIRED_MIGRATION_VERSION)
            .await
            .unwrap();
        let first = service.issue(DiscordUserId::new(321), "restricted").await.unwrap();
        assert_eq!(service.issue(DiscordUserId::new(321), "updated").await.unwrap(), first);
        for forbidden in [
            "CREATE TABLE mcguildlink.forbidden (id integer)",
            "DELETE FROM mcguildlink.link_requests",
            "DELETE FROM public._sqlx_migrations",
            "INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ('00000000-0000-0000-0000-000000000001', 'player')",
            "SELECT * FROM mcguildlink.block_groups",
        ] {
            let error = sqlx::query(forbidden).execute(&bot).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"));
        }
    }

    /// 同時発行の直列化を確認する。20 件の要求が同じコードを返し、別利用者には異なるコードを発行する。
    #[sqlx::test(migrations = "../../migrations")]
    async fn simultaneous_requests_return_one_reusable_code(pool: PgPool) {
        let service = DatabaseLinkCodes::new(pool);
        let results =
            futures::future::join_all((0..20).map(|_| service.issue(DiscordUserId::new(123), "concurrent"))).await;
        let first = results.first().unwrap().as_ref().unwrap();
        assert!(matches!(first, LinkCodeResult::Code(_)));
        for result in &results {
            assert_eq!(result.as_ref().unwrap(), first);
        }
        assert_eq!(&service.issue(DiscordUserId::new(123), "renamed").await.unwrap(), first);
        assert_ne!(
            &service.issue(DiscordUserId::new(124), "different").await.unwrap(),
            first
        );
    }

    /// ブロックによる発行拒否を確認する。既存コードの再表示と、コード削除後の新規発行をともに拒否する。
    #[sqlx::test(migrations = "../../migrations")]
    async fn blocked_user_cannot_issue_or_redisplay_code(pool: PgPool) {
        let service = DatabaseLinkCodes::new(pool.clone());
        service.issue(DiscordUserId::new(42), "blocked").await.unwrap();
        sqlx::query!(
            "INSERT INTO mcguildlink.block_groups (root_discord_account_id)
            SELECT id FROM mcguildlink.discord_accounts WHERE user_id = 42"
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query!(
            "INSERT INTO mcguildlink.blocked_discord_accounts (discord_account_id, block_group_id)
            SELECT root_discord_account_id, id FROM mcguildlink.block_groups"
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            service.issue(DiscordUserId::new(42), "blocked").await.unwrap(),
            LinkCodeResult::Blocked
        );
        sqlx::query!("DELETE FROM mcguildlink.link_requests")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            service.issue(DiscordUserId::new(42), "blocked").await.unwrap(),
            LinkCodeResult::Blocked
        );
    }

    /// 未使用コードの永続化を確認する。最大 Discord ID で発行し、サービス再生成・名前変更後も同じコードを返す。
    #[sqlx::test(migrations = "../../migrations")]
    async fn unused_code_survives_reconnection_and_name_change(pool: PgPool) {
        let service = DatabaseLinkCodes::new(pool.clone());
        let first = service.issue(DiscordUserId::new(u64::MAX), "before").await.unwrap();
        let LinkCodeResult::Code(code) = &first else {
            panic!("expected code")
        };
        assert_eq!(code.as_ref().len(), 8);
        assert_eq!(
            DatabaseLinkCodes::new(pool)
                .issue(DiscordUserId::new(u64::MAX), "after")
                .await
                .unwrap(),
            first
        );
    }
}
