use sqlx::PgConnection;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub struct AuditTarget {
    pub minecraft_uuid: Uuid,
    pub minecraft_name: String,
}

pub async fn lock_discord_account(connection: &mut PgConnection, user_id: u64) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT id FROM mcguildlink.discord_accounts WHERE user_id = $1::text::numeric FOR UPDATE",
        user_id.to_string()
    )
    .fetch_optional(connection)
    .await
}

pub async fn audit_targets_for_account(
    connection: &mut PgConnection,
    account_id: i64,
) -> Result<Vec<AuditTarget>, sqlx::Error> {
    sqlx::query_as::<_, AuditTarget>(
        "SELECT m.uuid AS minecraft_uuid, m.last_known_name AS minecraft_name \
         FROM mcguildlink.account_links l \
         JOIN mcguildlink.minecraft_accounts m ON m.id = l.minecraft_account_id \
         WHERE l.discord_account_id = $1 ORDER BY l.linked_at DESC, m.uuid",
    )
    .bind(account_id)
    .fetch_all(connection)
    .await
}

pub async fn delete_account_links(connection: &mut PgConnection, account_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM mcguildlink.account_links WHERE discord_account_id = $1",
        account_id
    )
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn delete_link_request(connection: &mut PgConnection, account_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM mcguildlink.link_requests WHERE discord_account_id = $1",
        account_id
    )
    .execute(connection)
    .await?;
    Ok(())
}

pub async fn record_member_leave(
    connection: &mut PgConnection,
    user_id: u64,
    username: &str,
    target: &AuditTarget,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO mcguildlink.audit_logs \
         (event_type, actor_type, actor_discord_user_id, actor_discord_username, \
          target_discord_user_id, target_discord_username, target_minecraft_uuid, target_minecraft_name) \
         VALUES ('member_leave_unlinked', 'discord_member', $1::text::numeric, $2, $1::text::numeric, $2, $3, $4)",
        user_id.to_string(),
        username,
        target.minecraft_uuid,
        target.minecraft_name
    )
    .execute(connection)
    .await?;
    Ok(())
}
