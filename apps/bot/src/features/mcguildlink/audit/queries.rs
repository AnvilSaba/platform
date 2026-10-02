use sqlx::{PgPool, types::Json};

use super::{DiscordSnapshot, MinecraftSnapshot, PendingAudit};

const BATCH_SIZE: i64 = 100;

pub(super) async fn pending(pool: &PgPool) -> Result<Vec<PendingAudit>, sqlx::Error> {
    sqlx::query_as!(
        PendingAudit,
        "SELECT a.id, a.event_type, a.occurred_at, a.actor_type, a.actor_minecraft_uuid,
                a.actor_minecraft_name, a.actor_discord_user_id::text AS actor_discord_user_id,
                a.actor_discord_username, a.target_discord_user_id::text AS \"target_discord_user_id!\",
                a.target_discord_username, a.target_minecraft_uuid, a.target_minecraft_name,
                a.related_discord_accounts AS \"related_discord_accounts: Json<Vec<DiscordSnapshot>>\",
                a.related_minecraft_accounts AS \"related_minecraft_accounts: Json<Vec<MinecraftSnapshot>>\"
         FROM mcguildlink.audit_outbox o
         JOIN mcguildlink.audit_logs a ON a.id = o.log_id
         WHERE NOT o.needs_attention AND o.next_attempt_at <= now()
         ORDER BY o.log_id LIMIT $1",
        BATCH_SIZE,
    )
    .fetch_all(pool)
    .await
}

pub(super) async fn delete_delivered(pool: &PgPool, event_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM mcguildlink.audit_outbox WHERE log_id = $1", event_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn record_failure(pool: &PgPool, event_id: i64, needs_attention: bool) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE mcguildlink.audit_outbox
         SET needs_attention = $2,
             next_attempt_at = now() + make_interval(secs =>
                 LEAST(3600.0, 60.0 * power(2.0, LEAST(retry_count, 6)))),
             retry_count = LEAST(retry_count, 2147483646) + 1
         WHERE log_id = $1",
        event_id,
        needs_attention,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 要対応の監査配送を再試行待ちへ戻す。ID未指定なら停止中の全件を対象とする。
pub async fn resume_stopped(pool: &PgPool, event_id: Option<i64>) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query!(
        "UPDATE mcguildlink.audit_outbox
         SET needs_attention = false, retry_count = 0, next_attempt_at = now()
         WHERE needs_attention AND ($1::bigint IS NULL OR log_id = $1)",
        event_id,
    )
    .execute(pool)
    .await?
    .rows_affected())
}
