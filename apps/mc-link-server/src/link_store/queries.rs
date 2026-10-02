use sqlx::PgConnection;
use uuid::Uuid;

pub(super) struct LinkRequest {
    pub(super) discord_id: i64,
    pub(super) user_id: String,
    pub(super) username: String,
}

pub(super) async fn lock_code(connection: &mut PgConnection, code: &str) -> Result<(), sqlx::Error> {
    sqlx::query!("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", code)
        .execute(connection)
        .await?;
    Ok(())
}

pub(super) async fn find_request(
    connection: &mut PgConnection,
    code: &str,
) -> Result<Option<LinkRequest>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT d.id, d.user_id::text AS \"user_id!\", d.last_known_username \
         FROM mcguildlink.link_requests r \
         JOIN mcguildlink.discord_accounts d ON d.id = r.discord_account_id \
         WHERE r.code = $1",
        code
    )
    .fetch_optional(connection)
    .await?;
    Ok(row.map(|row| LinkRequest {
        discord_id: row.id,
        user_id: row.user_id,
        username: row.last_known_username,
    }))
}

pub(super) async fn is_discord_blocked(connection: &mut PgConnection, discord_id: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts WHERE discord_account_id = $1) AS \"blocked!\"",
        discord_id
    ).fetch_one(connection).await
}

pub(super) async fn upsert_minecraft(
    connection: &mut PgConnection,
    uuid: Uuid,
    name: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ($1, $2) \
         ON CONFLICT (uuid) DO UPDATE SET last_known_name = EXCLUDED.last_known_name RETURNING id",
        uuid,
        name
    )
    .fetch_one(connection)
    .await
}

pub(super) async fn is_minecraft_blocked(
    connection: &mut PgConnection,
    minecraft_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT FROM mcguildlink.blocked_minecraft_accounts WHERE minecraft_account_id = $1) AS \"blocked!\"",
        minecraft_id
    ).fetch_one(connection).await
}

pub(super) async fn is_linked(
    connection: &mut PgConnection,
    discord_id: i64,
    minecraft_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT FROM mcguildlink.account_links WHERE discord_account_id = $1 AND minecraft_account_id = $2) AS \"linked!\"",
        discord_id, minecraft_id
    ).fetch_one(connection).await
}

pub(super) async fn insert_link(
    connection: &mut PgConnection,
    discord_id: i64,
    minecraft_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES ($1, $2)",
        discord_id,
        minecraft_id
    )
    .execute(connection)
    .await?;
    Ok(())
}

pub(super) async fn delete_request(connection: &mut PgConnection, discord_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM mcguildlink.link_requests WHERE discord_account_id = $1",
        discord_id
    )
    .execute(connection)
    .await?;
    Ok(())
}

pub(super) async fn record_link(
    connection: &mut PgConnection,
    uuid: Uuid,
    name: &str,
    user_id: &str,
    username: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO mcguildlink.audit_logs \
         (event_type, actor_type, actor_minecraft_uuid, actor_minecraft_name, \
          target_discord_user_id, target_discord_username, target_minecraft_uuid, target_minecraft_name) \
         VALUES ('link_succeeded', 'minecraft_player', $1, $2, $3::text::numeric, $4, $1, $2)",
        uuid,
        name,
        user_id,
        username
    )
    .execute(connection)
    .await?;
    Ok(())
}
