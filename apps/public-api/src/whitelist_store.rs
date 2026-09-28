use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct WhitelistVersion {
    pub(crate) revision: i64,
    pub(crate) last_modified_at: DateTime<Utc>,
    pub(crate) if_modified_since_safe: bool,
}

#[derive(Serialize)]
pub(crate) struct WhitelistEntry {
    uuid: Uuid,
    name: String,
}

pub(crate) struct WhitelistSnapshot {
    pub(crate) version: WhitelistVersion,
    pub(crate) entries: Vec<WhitelistEntry>,
}

/// 更新番号と一覧を同じ PostgreSQL スナップショットから取得する。
pub(crate) async fn snapshot(pool: &PgPool) -> Result<WhitelistSnapshot, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;
    let version = version(&mut *transaction).await?;
    let entries = read_entries(&mut transaction).await?;
    transaction.commit().await?;

    Ok(WhitelistSnapshot { version, entries })
}

pub(crate) async fn version<'e, E>(executor: E) -> Result<WhitelistVersion, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_as!(
        WhitelistVersion,
        "SELECT revision, last_modified_at, if_modified_since_safe
         FROM mcguildlink.whitelist_revision WHERE singleton",
    )
    .fetch_one(executor)
    .await
}

async fn read_entries(connection: &mut sqlx::PgConnection) -> Result<Vec<WhitelistEntry>, sqlx::Error> {
    sqlx::query_as!(
        WhitelistEntry,
        "SELECT DISTINCT m.uuid AS \"uuid!\", m.last_known_name AS \"name!\"
         FROM mcguildlink.account_links AS links
         JOIN mcguildlink.minecraft_accounts AS m ON m.id = links.minecraft_account_id
         WHERE NOT EXISTS (
             SELECT 1 FROM mcguildlink.blocked_discord_accounts AS blocked
             WHERE blocked.discord_account_id = links.discord_account_id
         ) AND NOT EXISTS (
             SELECT 1 FROM mcguildlink.blocked_minecraft_accounts AS blocked
             WHERE blocked.minecraft_account_id = m.id
         )
         ORDER BY m.uuid",
    )
    .fetch_all(connection)
    .await
}
