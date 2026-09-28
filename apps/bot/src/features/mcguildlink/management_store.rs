use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::app::AppError;

#[derive(Clone, Debug)]
pub struct Link {
    pub discord_user_id: String,
    pub discord_name: String,
    pub minecraft_uuid: Uuid,
    pub minecraft_name: String,
    pub linked_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct LinkManagement {
    pool: PgPool,
}

impl LinkManagement {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn by_discord(&self, user_id: u64) -> Result<Vec<Link>, AppError> {
        self.list(Some(user_id), None).await
    }

    pub async fn by_minecraft(&self, uuid: Uuid) -> Result<Vec<Link>, AppError> {
        self.list(None, Some(uuid)).await
    }

    pub async fn all(&self) -> Result<Vec<Link>, AppError> {
        self.list(None, None).await
    }

    async fn list(&self, user_id: Option<u64>, uuid: Option<Uuid>) -> Result<Vec<Link>, AppError> {
        let rows = sqlx::query(
            "SELECT d.user_id::text AS discord_user_id, d.last_known_username AS discord_name, \
                    m.uuid AS minecraft_uuid, m.last_known_name AS minecraft_name, l.linked_at \
             FROM mcguildlink.account_links l \
             JOIN mcguildlink.discord_accounts d ON d.id = l.discord_account_id \
             JOIN mcguildlink.minecraft_accounts m ON m.id = l.minecraft_account_id \
             WHERE ($1::text IS NULL OR d.user_id = $1::text::numeric) AND ($2::uuid IS NULL OR m.uuid = $2) \
             ORDER BY l.linked_at DESC, d.user_id, m.uuid",
        )
        .bind(user_id.map(|id| id.to_string()))
        .bind(uuid)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Link {
                    discord_user_id: row.try_get("discord_user_id")?,
                    discord_name: row.try_get("discord_name")?,
                    minecraft_uuid: row.try_get("minecraft_uuid")?,
                    minecraft_name: row.try_get("minecraft_name")?,
                    linked_at: row.try_get("linked_at")?,
                })
            })
            .collect()
    }

    pub async fn unlink(&self, user_id: u64, uuid: Uuid) -> Result<bool, AppError> {
        let result = sqlx::query(
            "DELETE FROM mcguildlink.account_links l USING mcguildlink.discord_accounts d, \
                    mcguildlink.minecraft_accounts m \
             WHERE l.discord_account_id = d.id AND l.minecraft_account_id = m.id \
               AND d.user_id = $1::text::numeric AND m.uuid = $2",
        )
        .bind(user_id.to_string())
        .bind(uuid)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() != 0)
    }

    /// 対象サーバーからの退出時だけ呼ぶ。紐付け・コード・監査記録をまとめて確定する。
    pub async fn member_left(&self, user_id: u64, username: &str) -> Result<Vec<Link>, AppError> {
        let mut tx = self.pool.begin().await?;
        let account_id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM mcguildlink.discord_accounts WHERE user_id = $1::text::numeric FOR UPDATE",
        )
        .bind(user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(account_id) = account_id else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(
            "SELECT m.uuid AS minecraft_uuid, m.last_known_name AS minecraft_name, l.linked_at \
             FROM mcguildlink.account_links l \
             JOIN mcguildlink.minecraft_accounts m ON m.id = l.minecraft_account_id \
             WHERE l.discord_account_id = $1 ORDER BY l.linked_at DESC, m.uuid",
        )
        .bind(account_id)
        .fetch_all(&mut *tx)
        .await?;
        let links: Vec<Link> = rows
            .into_iter()
            .map(|row| {
                Ok(Link {
                    discord_user_id: user_id.to_string(),
                    discord_name: username.to_owned(),
                    minecraft_uuid: row.try_get("minecraft_uuid")?,
                    minecraft_name: row.try_get("minecraft_name")?,
                    linked_at: row.try_get("linked_at")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()?;
        sqlx::query("DELETE FROM mcguildlink.account_links WHERE discord_account_id = $1")
            .bind(account_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM mcguildlink.link_requests WHERE discord_account_id = $1")
            .bind(account_id)
            .execute(&mut *tx)
            .await?;
        for link in &links {
            sqlx::query(
                "INSERT INTO mcguildlink.audit_logs \
                 (event_type, actor_type, actor_discord_user_id, actor_discord_username, \
                  target_discord_user_id, target_discord_username, target_minecraft_uuid, target_minecraft_name) \
                 VALUES ('member_leave_unlinked', 'discord_member', $1::text::numeric, $2, $1::text::numeric, $2, $3, $4)",
            )
            .bind(user_id.to_string())
            .bind(username)
            .bind(link.minecraft_uuid)
            .bind(&link.minecraft_name)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(links)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::mcguildlink::test_support;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    const FIRST: &str = "00000000-0000-0000-0000-000000000001";
    const SECOND: &str = "00000000-0000-0000-0000-000000000002";

    async fn seed(pool: &PgPool) {
        sqlx::raw_sql(&format!(
            "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice'), (20, 'bob');
             INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ('{FIRST}', 'First'), ('{SECOND}', 'Second');
             INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id)
                 VALUES (1, 1), (1, 2), (2, 1);
             INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'ALICECODE');"
        ))
        .execute(pool).await.unwrap();
    }

    async fn whitelist(pool: &PgPool) -> Value {
        let reader = sqlx::postgres::PgPoolOptions::new()
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET ROLE platform_public_api_runtime")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with((*pool.connect_options()).clone())
            .await
            .unwrap();
        let response = public_api::router(reader)
            .oneshot(Request::builder().uri("/whitelist.json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn lists_and_unlinks_only_the_selected_relationship(pool: PgPool) {
        seed(&pool).await;
        let store = LinkManagement::new(test_support::bot_pool(&pool).await);
        assert_eq!(store.by_discord(10).await.unwrap().len(), 2);
        assert_eq!(store.by_minecraft(FIRST.parse().unwrap()).await.unwrap().len(), 2);
        assert_eq!(store.all().await.unwrap().len(), 3);
        assert_eq!(
            whitelist(&pool).await,
            json!([
                {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
            ])
        );

        assert!(store.unlink(10, FIRST.parse().unwrap()).await.unwrap());
        assert!(!store.unlink(10, FIRST.parse().unwrap()).await.unwrap());
        assert_eq!(store.by_discord(10).await.unwrap().len(), 1);
        assert_eq!(store.by_minecraft(FIRST.parse().unwrap()).await.unwrap().len(), 1);
        assert_eq!(
            whitelist(&pool).await,
            json!([
                {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
            ])
        );
        assert!(store.unlink(20, FIRST.parse().unwrap()).await.unwrap());
        assert_eq!(whitelist(&pool).await, json!([{"uuid": SECOND, "name": "Second"}]));
        let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.audit_logs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(audit_count, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn leaving_removes_links_and_code_and_enqueues_audit(pool: PgPool) {
        seed(&pool).await;
        let store = LinkManagement::new(test_support::bot_pool(&pool).await);
        let removed = store.member_left(10, "alice-now").await.unwrap();
        assert_eq!(removed.len(), 2);
        assert!(store.by_discord(10).await.unwrap().is_empty());
        assert_eq!(store.by_discord(20).await.unwrap().len(), 1);
        let code_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.link_requests")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(code_count, 0);
        let audit: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT a.event_type, a.actor_discord_user_id::text, a.target_minecraft_name
             FROM mcguildlink.audit_logs a JOIN mcguildlink.audit_outbox o ON o.log_id = a.id
             ORDER BY a.target_minecraft_name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            audit,
            vec![
                ("member_leave_unlinked".into(), "10".into(), "First".into()),
                ("member_leave_unlinked".into(), "10".into(), "Second".into()),
            ]
        );
        assert_eq!(whitelist(&pool).await, json!([{"uuid": FIRST, "name": "First"}]));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn audit_failure_rolls_back_leave(pool: PgPool) {
        seed(&pool).await;
        sqlx::raw_sql(
            "CREATE FUNCTION mcguildlink.reject_test_audit() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN RAISE EXCEPTION 'audit unavailable'; END $$;
            CREATE TRIGGER reject_test_audit BEFORE INSERT ON mcguildlink.audit_logs
            FOR EACH ROW EXECUTE FUNCTION mcguildlink.reject_test_audit();",
        )
        .execute(&pool)
        .await
        .unwrap();
        let store = LinkManagement::new(test_support::bot_pool(&pool).await);
        assert!(store.member_left(10, "alice").await.is_err());
        assert_eq!(store.by_discord(10).await.unwrap().len(), 2);
        let code: String = sqlx::query_scalar("SELECT code FROM mcguildlink.link_requests")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(code, "ALICECODE");
        assert_eq!(
            whitelist(&pool).await,
            json!([
                {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
            ])
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn leaving_with_only_unused_code_removes_it_without_audit(pool: PgPool) {
        sqlx::raw_sql(
            "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice');
            INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'ALICECODE');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let store = LinkManagement::new(test_support::bot_pool(&pool).await);
        assert!(store.member_left(10, "alice").await.unwrap().is_empty());
        let code_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.link_requests")
            .fetch_one(&pool)
            .await
            .unwrap();
        let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.audit_logs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((code_count, audit_count), (0, 0));
    }
}
