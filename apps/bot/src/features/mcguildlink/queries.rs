use sqlx::PgConnection;

/// 同一利用者の発行要求は、呼び出し元のトランザクションで行ロックを保持して直列化する。
pub async fn upsert_discord_account(
    connection: &mut PgConnection,
    user_id: u64,
    username: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES ($1::text::numeric, $2)
             ON CONFLICT (user_id) DO UPDATE SET last_known_username = EXCLUDED.last_known_username RETURNING id",
        user_id.to_string(),
        username
    )
    .fetch_one(connection)
    .await
}

pub async fn is_discord_blocked(connection: &mut PgConnection, account: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts WHERE discord_account_id = $1) AS \"blocked!\"",
        account
    ).fetch_one(connection).await
}

pub async fn unused_code(connection: &mut PgConnection, account: i64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT code FROM mcguildlink.link_requests WHERE discord_account_id = $1",
        account
    )
    .fetch_optional(connection)
    .await
}

pub async fn reserve_code(connection: &mut PgConnection, account: i64, code: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES ($1, $2) ON CONFLICT (code) DO NOTHING",
        account,
        code
    ).execute(connection).await?;
    Ok(result.rows_affected() == 1)
}
