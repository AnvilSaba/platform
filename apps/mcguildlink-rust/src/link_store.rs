use std::sync::Arc;

use sqlx::{PgPool, Row};
use tokio::runtime::Runtime;

use crate::{AppResult, session::SessionProfile};

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
const REQUIRED_MIGRATIONS: &[i64] = &[20260926184758, 20260927120000];

#[derive(Clone)]
pub(crate) struct LinkStore {
    pool: PgPool,
    runtime: Arc<Runtime>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LinkResult {
    InvalidCode,
    Blocked,
    AlreadyLinked,
    Success(String),
}

impl LinkStore {
    pub(crate) fn connect() -> AppResult<Self> {
        let runtime = Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build()?);
        let pool = runtime.block_on(platform_database::DatabaseConfig::from_env()?.connect())?;
        runtime.block_on(platform_database::check_migrations(
            &pool,
            &MIGRATIONS,
            REQUIRED_MIGRATIONS,
        ))?;
        Ok(Self { pool, runtime })
    }

    pub(crate) fn consume(&self, code: &str, player: &SessionProfile) -> AppResult<LinkResult> {
        Ok(self.runtime.block_on(consume(&self.pool, code, player))?)
    }
}

async fn consume(pool: &PgPool, code: &str, player: &SessionProfile) -> Result<LinkResult, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(code)
        .execute(&mut *tx)
        .await?;
    let request = sqlx::query(
        "SELECT d.id, d.user_id::text AS user_id, d.last_known_username \
         FROM mcguildlink.link_requests r \
         JOIN mcguildlink.discord_accounts d ON d.id = r.discord_account_id \
         WHERE r.code = $1",
    )
    .bind(code)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(request) = request else {
        return Ok(LinkResult::InvalidCode);
    };
    let discord_id: i64 = request.get("id");
    let user_id: String = request.get("user_id");
    let username: String = request.get("last_known_username");

    let blocked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts WHERE discord_account_id = $1)",
    )
    .bind(discord_id)
    .fetch_one(&mut *tx)
    .await?;
    if blocked {
        return Ok(LinkResult::Blocked);
    }

    let minecraft_id: i64 = sqlx::query_scalar(
        "INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ($1, $2) \
         ON CONFLICT (uuid) DO UPDATE SET last_known_name = EXCLUDED.last_known_name RETURNING id",
    )
    .bind(player.id)
    .bind(player.name.as_ref())
    .fetch_one(&mut *tx)
    .await?;

    let blocked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM mcguildlink.blocked_minecraft_accounts WHERE minecraft_account_id = $1)",
    )
    .bind(minecraft_id)
    .fetch_one(&mut *tx)
    .await?;
    if blocked {
        return Ok(LinkResult::Blocked);
    }

    let linked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT FROM mcguildlink.account_links WHERE discord_account_id = $1 AND minecraft_account_id = $2)",
    )
    .bind(discord_id)
    .bind(minecraft_id)
    .fetch_one(&mut *tx)
    .await?;
    if linked {
        return Ok(LinkResult::AlreadyLinked);
    }

    sqlx::query("INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES ($1, $2)")
        .bind(discord_id)
        .bind(minecraft_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM mcguildlink.link_requests WHERE discord_account_id = $1")
        .bind(discord_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO mcguildlink.audit_logs \
         (event_type, actor_type, actor_minecraft_uuid, actor_minecraft_name, \
          target_discord_user_id, target_discord_username, target_minecraft_uuid, target_minecraft_name) \
         VALUES ('link_succeeded', 'minecraft_player', $1, $2, $3::numeric, $4, $1, $2)",
    )
    .bind(player.id)
    .bind(player.name.as_ref())
    .bind(&user_id)
    .bind(&username)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(LinkResult::Success(username))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Name;
    use uuid::Uuid;

    async fn restricted_pool(pool: &PgPool) -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET ROLE platform_mcguildlink_runtime")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with((*pool.connect_options()).clone())
            .await
            .unwrap()
    }

    async fn issue(pool: &PgPool, user: i64, code: &str) {
        sqlx::query(
            "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES ($1, 'DiscordUser')",
        )
        .bind(user)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO mcguildlink.link_requests (discord_account_id, code) SELECT id, $2 FROM mcguildlink.discord_accounts WHERE user_id = $1")
            .bind(user).bind(code).execute(pool).await.unwrap();
    }

    fn player() -> SessionProfile {
        SessionProfile {
            id: Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap(),
            name: Name::try_new("TestPlayer").unwrap(),
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn success_consumes_code_and_keeps_audit_and_outbox_together(pool: PgPool) {
        issue(&pool, 10, "CODE0001").await;
        issue(&pool, 20, "CODE0002").await;
        let restricted = restricted_pool(&pool).await;
        assert_eq!(
            consume(&restricted, "WRONG", &player()).await.unwrap(),
            LinkResult::InvalidCode
        );
        assert_eq!(
            consume(&restricted, "CODE0001", &player()).await.unwrap(),
            LinkResult::Success("DiscordUser".into())
        );
        assert_eq!(
            consume(&restricted, "CODE0001", &player()).await.unwrap(),
            LinkResult::InvalidCode
        );
        sqlx::query("INSERT INTO mcguildlink.link_requests (discord_account_id, code) SELECT id, 'CODE0003' FROM mcguildlink.discord_accounts WHERE user_id = 10")
            .execute(&pool).await.unwrap();
        assert_eq!(
            consume(&restricted, "CODE0003", &player()).await.unwrap(),
            LinkResult::AlreadyLinked
        );
        assert_eq!(
            consume(&restricted, "CODE0002", &player()).await.unwrap(),
            LinkResult::Success("DiscordUser".into())
        );
        let counts: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM mcguildlink.account_links),
                    (SELECT count(*) FROM mcguildlink.link_requests),
                    (SELECT count(*) FROM mcguildlink.audit_logs),
                    (SELECT count(*) FROM mcguildlink.audit_outbox)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (2, 1, 2, 2));
        let log: (String, String, String) = sqlx::query_as(
            "SELECT event_type, actor_type, target_discord_user_id::text FROM mcguildlink.audit_logs ORDER BY id LIMIT 1"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(log, ("link_succeeded".into(), "minecraft_player".into(), "10".into()));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn rejected_or_failed_link_retains_code_and_never_leaves_partial_audit(pool: PgPool) {
        issue(&pool, 10, "CODE0001").await;
        let restricted = restricted_pool(&pool).await;
        assert_eq!(
            consume(&restricted, "CODE0001", &player()).await.unwrap(),
            LinkResult::Success("DiscordUser".into())
        );
        issue(&pool, 20, "CODE0002").await;
        assert_eq!(
            consume(&restricted, "CODE0002", &player()).await.unwrap(),
            LinkResult::Success("DiscordUser".into())
        );
        issue(&pool, 30, "CODE0003").await;
        sqlx::query("INSERT INTO mcguildlink.block_groups (root_discord_account_id) SELECT id FROM mcguildlink.discord_accounts WHERE user_id = 30")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO mcguildlink.blocked_discord_accounts SELECT root_discord_account_id, id FROM mcguildlink.block_groups")
            .execute(&pool).await.unwrap();
        assert_eq!(
            consume(&restricted, "CODE0003", &player()).await.unwrap(),
            LinkResult::Blocked
        );
        issue(&pool, 40, "CODE0004").await;
        sqlx::query("CREATE FUNCTION mcguildlink.fail_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'forced failure'; END $$")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_audit BEFORE INSERT ON mcguildlink.audit_logs FOR EACH ROW EXECUTE FUNCTION mcguildlink.fail_audit()")
            .execute(&pool).await.unwrap();
        assert!(consume(&restricted, "CODE0004", &player()).await.is_err());
        let counts: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM mcguildlink.account_links WHERE discord_account_id = (SELECT id FROM mcguildlink.discord_accounts WHERE user_id = 40)),
                    (SELECT count(*) FROM mcguildlink.link_requests WHERE code IN ('CODE0003', 'CODE0004')),
                    (SELECT count(*) FROM mcguildlink.audit_outbox)"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (0, 2, 2));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn same_code_cannot_be_consumed_twice_and_role_cannot_change_schema(pool: PgPool) {
        issue(&pool, 10, "CODE0001").await;
        let restricted = restricted_pool(&pool).await;
        let player = player();
        let (left, right) = tokio::join!(
            consume(&restricted, "CODE0001", &player),
            consume(&restricted, "CODE0001", &player)
        );
        let results = [left.unwrap(), right.unwrap()];
        assert!(results.contains(&LinkResult::Success("DiscordUser".into())));
        assert!(results.contains(&LinkResult::InvalidCode));
        let error = sqlx::query("CREATE TABLE mcguildlink.forbidden (id integer)")
            .execute(&restricted)
            .await
            .unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"));
        let error = sqlx::query("DELETE FROM mcguildlink.audit_logs")
            .execute(&restricted)
            .await
            .unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"));
    }
}
