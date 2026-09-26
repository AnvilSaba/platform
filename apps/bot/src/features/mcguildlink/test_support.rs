use sqlx::PgPool;

pub async fn bot_pool(pool: &PgPool) -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query!("SET ROLE platform_bot_runtime")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap()
}

/// ブロック拒否テスト用に、発行済みの利用者 42 をブロックする。
pub async fn block_test_account(pool: &PgPool) {
    sqlx::query!(
        "INSERT INTO mcguildlink.block_groups (root_discord_account_id)
            SELECT id FROM mcguildlink.discord_accounts WHERE user_id = 42"
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query!(
        "INSERT INTO mcguildlink.blocked_discord_accounts (discord_account_id, block_group_id)
            SELECT root_discord_account_id, id FROM mcguildlink.block_groups"
    )
    .execute(pool)
    .await
    .unwrap();
}

/// 未使用コードを除去し、新規発行の拒否を検証できる状態にする。
pub async fn remove_test_codes(pool: &PgPool) {
    sqlx::query!("DELETE FROM mcguildlink.link_requests")
        .execute(pool)
        .await
        .unwrap();
}
