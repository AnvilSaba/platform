use sqlx::PgPool;

use crate::{AppResult, session::SessionProfile};

mod queries;

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
const REQUIRED_MIGRATIONS: &[i64] = &[20260926184758, 20260927120000, 20260929180000];

#[derive(Clone)]
pub(crate) struct LinkStore {
    pool: PgPool,
}

pub(crate) trait CodeLinker: Send + Sync {
    async fn consume(&self, code: &str, player: &SessionProfile) -> AppResult<LinkResult>;
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LinkResult {
    InvalidCode,
    Blocked,
    AlreadyLinked,
    Success(String),
}

impl LinkStore {
    pub(crate) async fn connect() -> AppResult<Self> {
        let pool = platform_database::DatabaseConfig::from_env()?.connect().await?;
        platform_database::check_migrations(&pool, &MIGRATIONS, REQUIRED_MIGRATIONS).await?;
        Ok(Self { pool })
    }
}

impl CodeLinker for LinkStore {
    async fn consume(&self, code: &str, player: &SessionProfile) -> AppResult<LinkResult> {
        Ok(consume(&self.pool, code, player).await?)
    }
}

async fn consume(pool: &PgPool, code: &str, player: &SessionProfile) -> Result<LinkResult, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT mcguildlink.serialize_account_changes()")
        .execute(&mut *tx)
        .await?;
    queries::lock_code(&mut tx, code).await?;
    let request = queries::find_request(&mut tx, code).await?;
    let Some(request) = request else {
        return Ok(LinkResult::InvalidCode);
    };
    if queries::is_discord_blocked(&mut tx, request.discord_id).await? {
        return Ok(LinkResult::Blocked);
    }

    let minecraft_id = queries::upsert_minecraft(&mut tx, player.id, player.name.as_ref()).await?;
    if queries::is_minecraft_blocked(&mut tx, minecraft_id).await? {
        return Ok(LinkResult::Blocked);
    }
    if queries::is_linked(&mut tx, request.discord_id, minecraft_id).await? {
        return Ok(LinkResult::AlreadyLinked);
    }

    queries::insert_link(&mut tx, request.discord_id, minecraft_id).await?;
    queries::delete_request(&mut tx, request.discord_id).await?;
    queries::record_link(
        &mut tx,
        player.id,
        player.name.as_ref(),
        &request.user_id,
        &request.username,
    )
    .await?;
    tx.commit().await?;
    Ok(LinkResult::Success(request.username))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::identity::Name;

    async fn restricted_pool(pool: &PgPool) -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET ROLE platform_mc_link_server_runtime")
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

    #[sqlx::test(migrations = "../../migrations")]
    async fn blocked_minecraft_account_rejects_link_without_consuming_code(pool: PgPool) {
        issue(&pool, 10, "CODE0001").await;
        sqlx::query("INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ($1, 'TestPlayer')")
            .bind(player().id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mcguildlink.block_groups (root_discord_account_id) SELECT id FROM mcguildlink.discord_accounts WHERE user_id = 10")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO mcguildlink.blocked_minecraft_accounts (minecraft_account_id, block_group_id) SELECT m.id, g.id FROM mcguildlink.minecraft_accounts m CROSS JOIN mcguildlink.block_groups g")
            .execute(&pool).await.unwrap();
        let restricted = restricted_pool(&pool).await;
        assert_eq!(
            consume(&restricted, "CODE0001", &player()).await.unwrap(),
            LinkResult::Blocked
        );
        let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM mcguildlink.link_requests), (SELECT count(*) FROM mcguildlink.account_links), (SELECT count(*) FROM mcguildlink.audit_logs)")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (1, 0, 0));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn outbox_failure_rolls_back_link_code_and_audit(pool: PgPool) {
        issue(&pool, 10, "CODE0001").await;
        sqlx::query("CREATE FUNCTION mcguildlink.fail_outbox() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'forced outbox failure'; END $$")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_outbox BEFORE INSERT ON mcguildlink.audit_outbox FOR EACH ROW EXECUTE FUNCTION mcguildlink.fail_outbox()")
            .execute(&pool).await.unwrap();
        let restricted = restricted_pool(&pool).await;
        assert!(consume(&restricted, "CODE0001", &player()).await.is_err());
        let counts: (i64, i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM mcguildlink.link_requests), (SELECT count(*) FROM mcguildlink.account_links), (SELECT count(*) FROM mcguildlink.audit_logs), (SELECT count(*) FROM mcguildlink.audit_outbox)")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(counts, (1, 0, 0, 0));
    }
}
