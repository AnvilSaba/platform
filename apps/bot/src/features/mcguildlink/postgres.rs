use super::{
    code_generator::RandomLinkCodeGenerator,
    ports::{LinkCodeGenerator, LinkCodeResult, LinkCodes},
    queries,
};
use crate::app::AppError;
use serenity::async_trait;
use sqlx::{PgConnection, PgPool};

const MAX_CODE_ALLOCATION_ATTEMPTS: usize = 16;

#[derive(Clone)]
pub struct PostgresLinkCodes<G = RandomLinkCodeGenerator> {
    pool: PgPool,
    generator: G,
}

impl PostgresLinkCodes {
    pub fn new(pool: PgPool) -> Self {
        Self::with_generator(pool, RandomLinkCodeGenerator)
    }
}

impl<G: LinkCodeGenerator> PostgresLinkCodes<G> {
    pub fn with_generator(pool: PgPool, generator: G) -> Self {
        Self { pool, generator }
    }

    async fn issue_in_transaction(
        &self,
        connection: &mut PgConnection,
        user_id: u64,
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
impl<G: LinkCodeGenerator> LinkCodes for PostgresLinkCodes<G> {
    async fn issue(&self, user_id: u64, username: &str) -> Result<LinkCodeResult, AppError> {
        let mut tx = self.pool.begin().await?;
        let result = self.issue_in_transaction(&mut tx, user_id, username).await?;
        tx.commit().await?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

    struct FixedCodes(Mutex<VecDeque<&'static str>>);

    impl FixedCodes {
        fn new(codes: &[&'static str]) -> Self {
            Self(Mutex::new(codes.iter().copied().collect()))
        }
    }

    impl LinkCodeGenerator for FixedCodes {
        fn generate(&self) -> String {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected code generation")
                .into()
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn colliding_code_is_retried_without_changing_another_users_code(pool: PgPool) {
        let first = PostgresLinkCodes::with_generator(pool.clone(), FixedCodes::new(&["AC234679"]));
        assert_eq!(
            first.issue(10, "first").await.unwrap(),
            LinkCodeResult::Code("AC234679".into())
        );
        let second = PostgresLinkCodes::with_generator(pool, FixedCodes::new(&["AC234679", "KMNPQRTU"]));
        assert_eq!(
            second.issue(20, "second").await.unwrap(),
            LinkCodeResult::Code("KMNPQRTU".into())
        );
        assert_eq!(
            first.issue(10, "first").await.unwrap(),
            LinkCodeResult::Code("AC234679".into())
        );
        assert_eq!(
            second.issue(20, "second").await.unwrap(),
            LinkCodeResult::Code("KMNPQRTU".into())
        );
    }

    async fn bot_pool(pool: &PgPool) -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query!("SET ROLE mcguildlink_bot").execute(connection).await?;
                    Ok(())
                })
            })
            .connect_with((*pool.connect_options()).clone())
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn bot_role_can_issue_but_cannot_change_schema_or_unrelated_data(pool: PgPool) {
        let bot = bot_pool(&pool).await;
        let service = PostgresLinkCodes::new(bot.clone());
        platform_database::check_migrations(&bot, &MIGRATIONS, crate::REQUIRED_MIGRATION_VERSION)
            .await
            .unwrap();
        let first = service.issue(321, "restricted").await.unwrap();
        assert_eq!(service.issue(321, "updated").await.unwrap(), first);
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn simultaneous_requests_return_one_reusable_code(pool: PgPool) {
        let service = PostgresLinkCodes::new(pool);
        let results = futures::future::join_all((0..20).map(|_| service.issue(123, "concurrent"))).await;
        let first = results.first().unwrap().as_ref().unwrap();
        assert!(matches!(first, LinkCodeResult::Code(_)));
        for result in &results {
            assert_eq!(result.as_ref().unwrap(), first);
        }
        assert_eq!(&service.issue(123, "renamed").await.unwrap(), first);
        assert_ne!(&service.issue(124, "different").await.unwrap(), first);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn blocked_user_cannot_issue_or_redisplay_code(pool: PgPool) {
        let service = PostgresLinkCodes::new(pool.clone());
        service.issue(42, "blocked").await.unwrap();
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
        assert_eq!(service.issue(42, "blocked").await.unwrap(), LinkCodeResult::Blocked);
        sqlx::query!("DELETE FROM mcguildlink.link_requests")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(service.issue(42, "blocked").await.unwrap(), LinkCodeResult::Blocked);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn unused_code_survives_reconnection_and_name_change(pool: PgPool) {
        let service = PostgresLinkCodes::new(pool.clone());
        let first = service.issue(u64::MAX, "before").await.unwrap();
        let LinkCodeResult::Code(code) = &first else {
            panic!("expected code")
        };
        assert_eq!(code.len(), 8);
        assert_eq!(
            PostgresLinkCodes::new(pool).issue(u64::MAX, "after").await.unwrap(),
            first
        );
    }
}
