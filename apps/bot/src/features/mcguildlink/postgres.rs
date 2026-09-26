use super::ports::{LinkCodeResult, LinkCodes};
use crate::app::AppError;
use rand::RngExt;
use serenity::async_trait;
use sqlx::PgPool;

#[derive(Clone)]
pub struct PostgresLinkCodes {
    pool: PgPool,
}

impl PostgresLinkCodes {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LinkCodes for PostgresLinkCodes {
    async fn issue(&self, user_id: u64, username: &str) -> Result<LinkCodeResult, AppError> {
        let mut tx = self.pool.begin().await?;
        // UPSERT の行ロックで同一アカウントの要求を直列化する。
        let account = sqlx::query_scalar!(
            "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES ($1::text::numeric, $2)
             ON CONFLICT (user_id) DO UPDATE SET last_known_username = EXCLUDED.last_known_username RETURNING id",
            user_id.to_string(),
            username
        )
        .fetch_one(&mut *tx)
        .await?;
        let blocked = sqlx::query_scalar!(
            "SELECT EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts WHERE discord_account_id = $1) AS \"blocked!\"",
            account
        )
        .fetch_one(&mut *tx)
        .await?;
        if blocked {
            tx.commit().await?;
            return Ok(LinkCodeResult::Blocked);
        }
        let existing: Option<String> = sqlx::query_scalar!(
            "SELECT code FROM mcguildlink.link_requests WHERE discord_account_id = $1",
            account
        )
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(code) = existing {
            tx.commit().await?;
            return Ok(LinkCodeResult::Code(code));
        }
        const CHARS: &[u8] = b"ACDEFGHJKMNPQRTUVWXYZacdefghjkmnpqrtuvwxyz234679";
        for _ in 0..16 {
            let code: String = (0..8)
                .map(|_| CHARS[rand::rng().random_range(0..CHARS.len())] as char)
                .collect();
            let inserted = sqlx::query!(
                "INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES ($1, $2) ON CONFLICT (code) DO NOTHING",
                account,
                code
            ).execute(&mut *tx).await?;
            if inserted.rows_affected() == 1 {
                tx.commit().await?;
                return Ok(LinkCodeResult::Code(code));
            }
        }
        Err(anyhow::anyhow!("Could not allocate a unique link code"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

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
        platform_database::check_migrations(&bot, &MIGRATIONS, 20260926184758)
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
