use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

pub const FIRST: &str = "00000000-0000-0000-0000-000000000001";
pub const SECOND: &str = "00000000-0000-0000-0000-000000000002";

pub async fn seed_linked_accounts(pool: &PgPool) {
    sqlx::raw_sql(&format!(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice'), (20, 'bob');
         INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ('{FIRST}', 'First'), ('{SECOND}', 'Second');
         INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id)
             VALUES (1, 1), (1, 2), (2, 1);
         INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'ALICECODE');"
    ))
    .execute(pool).await.unwrap();
}

pub async fn whitelist(pool: &PgPool) -> Value {
    let reader = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query!("SET ROLE platform_public_api_runtime")
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
