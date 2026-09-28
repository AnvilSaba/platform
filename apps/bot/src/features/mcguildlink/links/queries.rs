use sqlx::PgPool;
use uuid::Uuid;

use super::model::Link;

pub async fn list(pool: &PgPool, user_id: Option<u64>, uuid: Option<Uuid>) -> Result<Vec<Link>, sqlx::Error> {
    let user_id = user_id.map(|id| id.to_string());
    sqlx::query_as!(
        Link,
        "SELECT d.user_id::text AS \"discord_user_id!\", d.last_known_username AS \"discord_name!\", \
                m.uuid AS \"minecraft_uuid!\", m.last_known_name AS \"minecraft_name!\", l.linked_at AS \"linked_at!\" \
         FROM mcguildlink.account_links l \
         JOIN mcguildlink.discord_accounts d ON d.id = l.discord_account_id \
         JOIN mcguildlink.minecraft_accounts m ON m.id = l.minecraft_account_id \
         WHERE ($1::text IS NULL OR d.user_id = $1::text::numeric) AND ($2::uuid IS NULL OR m.uuid = $2) \
         ORDER BY l.linked_at DESC, d.user_id, m.uuid",
        user_id,
        uuid
    )
    .fetch_all(pool)
    .await
}

pub async fn unlink(pool: &PgPool, user_id: u64, uuid: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM mcguildlink.account_links l USING mcguildlink.discord_accounts d, \
                mcguildlink.minecraft_accounts m \
         WHERE l.discord_account_id = d.id AND l.minecraft_account_id = m.id \
           AND d.user_id = $1::text::numeric AND m.uuid = $2",
        user_id.to_string(),
        uuid
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() != 0)
}
