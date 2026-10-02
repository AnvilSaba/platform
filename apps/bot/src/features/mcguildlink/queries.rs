use super::types::{DiscordAccountId, DiscordUserId, LinkCode};
use sqlx::PgConnection;

/// Discord アカウントを登録し、既存の場合は最終確認したユーザー名を更新する。
/// 同一利用者の発行要求は、呼び出し元のトランザクションで行ロックを保持して直列化する。
pub async fn upsert_discord_account(
    connection: &mut PgConnection,
    user_id: DiscordUserId,
    username: &str,
) -> Result<DiscordAccountId, sqlx::Error> {
    let id = sqlx::query_scalar!(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES ($1::text::numeric, $2)
             ON CONFLICT (user_id) DO UPDATE SET last_known_username = EXCLUDED.last_known_username RETURNING id",
        user_id.into_inner().to_string(),
        username
    )
    .fetch_one(connection)
    .await?;
    Ok(DiscordAccountId::new(id))
}

/// ブロックグループに所属するアカウントかを確認する。
pub async fn is_discord_blocked(connection: &mut PgConnection, account: DiscordAccountId) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts WHERE discord_account_id = $1) AS \"blocked!\"",
        account.into_inner()
    ).fetch_one(connection).await
}

/// 未使用コードを取得して再表示に使う。有効期限は設けない。
pub async fn unused_code(
    connection: &mut PgConnection,
    account: DiscordAccountId,
) -> Result<Option<LinkCode>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT code FROM mcguildlink.link_requests WHERE discord_account_id = $1",
        account.into_inner()
    )
    .fetch_optional(connection)
    .await
    .map(|code| code.map(LinkCode::new))
}

/// コードを保存する。一意制約でコードが衝突した場合だけ false を返す。
pub async fn reserve_code(
    connection: &mut PgConnection,
    account: DiscordAccountId,
    code: &LinkCode,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES ($1, $2) ON CONFLICT (code) DO NOTHING",
        account.into_inner(),
        code.as_ref()
    ).execute(connection).await?;
    Ok(result.rows_affected() == 1)
}
