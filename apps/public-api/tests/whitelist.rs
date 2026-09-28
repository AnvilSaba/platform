use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use tower::ServiceExt;

use public_api::router;

async fn reader(pool: &PgPool) -> PgPool {
    PgPoolOptions::new()
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
        .unwrap()
}

async fn get(app: &Router) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/whitelist.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    if status == StatusCode::OK {
        assert_eq!(response.headers()["content-type"], "application/json");
    }
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        if status != StatusCode::OK || body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}

async fn add_discord(pool: &PgPool, user_id: i64) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES ($1, 'Discord') RETURNING id",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn add_minecraft(pool: &PgPool, uuid: &str, name: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ($1::uuid, $2) RETURNING id",
    )
    .bind(uuid)
    .bind(name)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn link(pool: &PgPool, discord_id: i64, minecraft_id: i64) {
    sqlx::query("INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES ($1, $2)")
        .bind(discord_id)
        .bind(minecraft_id)
        .execute(pool)
        .await
        .unwrap();
}

const FIRST: &str = "00000000-0000-0000-0000-000000000001";
const SECOND: &str = "00000000-0000-0000-0000-000000000002";

#[sqlx::test(migrations = "../../migrations")]
async fn whitelist_keeps_legacy_json_and_refreshes_after_links_names_and_deletions(pool: PgPool) {
    let app = router(reader(&pool).await);
    assert_eq!(get(&app).await, (StatusCode::OK, json!([])));
    let discord_one = add_discord(&pool, 1).await;
    let discord_two = add_discord(&pool, 2).await;
    let second = add_minecraft(&pool, SECOND, "MiXeD").await;
    let first = add_minecraft(&pool, FIRST, "First").await;
    link(&pool, discord_one, second).await;
    link(&pool, discord_two, second).await;
    link(&pool, discord_one, first).await;
    let expected = json!([{"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "MiXeD"}]);
    assert_eq!(get(&app).await, (StatusCode::OK, expected.clone()));
    assert_eq!(get(&app).await, (StatusCode::OK, expected));

    sqlx::query("UPDATE mcguildlink.minecraft_accounts SET last_known_name = 'Renamed' WHERE id = $1")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        get(&app).await.1,
        json!([{"uuid": FIRST, "name": "Renamed"}, {"uuid": SECOND, "name": "MiXeD"}])
    );
    sqlx::query("DELETE FROM mcguildlink.account_links WHERE minecraft_account_id = $1")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([{"uuid": SECOND, "name": "MiXeD"}]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn whitelist_reflects_discord_and_minecraft_blocks_and_unblocks(pool: PgPool) {
    let app = router(reader(&pool).await);
    let discord = add_discord(&pool, 1).await;
    let other_discord = add_discord(&pool, 2).await;
    let minecraft = add_minecraft(&pool, FIRST, "First").await;
    link(&pool, discord, minecraft).await;
    link(&pool, other_discord, minecraft).await;
    assert_eq!(get(&app).await.1, json!([{"uuid": FIRST, "name": "First"}]));
    let group: i64 =
        sqlx::query_scalar("INSERT INTO mcguildlink.block_groups (root_discord_account_id) VALUES ($1) RETURNING id")
            .bind(discord)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO mcguildlink.blocked_discord_accounts VALUES ($1, $2)")
        .bind(discord)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([{"uuid": FIRST, "name": "First"}]));
    sqlx::query("INSERT INTO mcguildlink.blocked_discord_accounts VALUES ($1, $2)")
        .bind(other_discord)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([]));
    sqlx::query("DELETE FROM mcguildlink.blocked_discord_accounts WHERE discord_account_id = $1")
        .bind(discord)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([{"uuid": FIRST, "name": "First"}]));
    sqlx::query("INSERT INTO mcguildlink.blocked_minecraft_accounts VALUES ($1, $2)")
        .bind(minecraft)
        .bind(group)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([]));
    sqlx::query("DELETE FROM mcguildlink.blocked_minecraft_accounts WHERE minecraft_account_id = $1")
        .bind(minecraft)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([{"uuid": FIRST, "name": "First"}]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn uncommitted_changes_are_not_published_and_rollback_keeps_previous_json(pool: PgPool) {
    let app = router(reader(&pool).await);
    assert_eq!(get(&app).await.1, json!([]));
    let discord = add_discord(&pool, 1).await;
    let minecraft = add_minecraft(&pool, FIRST, "First").await;
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO mcguildlink.account_links VALUES ($1, $2, now())")
        .bind(discord)
        .bind(minecraft)
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([]));
    transaction.rollback().await.unwrap();
    assert_eq!(get(&app).await.1, json!([]));
    link(&pool, discord, minecraft).await;
    assert_eq!(get(&app).await.1, json!([{"uuid": FIRST, "name": "First"}]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn concurrent_commit_never_mixes_revision_with_a_newer_list(pool: PgPool) {
    let app = router(reader(&pool).await);
    assert_eq!(get(&app).await.1, json!([]));
    let discord = add_discord(&pool, 1).await;
    let first = add_minecraft(&pool, FIRST, "First").await;
    let second = add_minecraft(&pool, SECOND, "Second").await;
    link(&pool, discord, first).await;

    // 一覧の SELECT だけを待たせ、API が番号を読んだ後に別の変更を commit する。
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE mcguildlink.account_links IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES ($1, $2)")
        .bind(discord)
        .bind(second)
        .execute(&mut *writer)
        .await
        .unwrap();
    let request_app = app.clone();
    let pending = tokio::spawn(async move { get(&request_app).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT FROM pg_locks AS locks
                    JOIN pg_stat_activity AS activity USING (pid)
                    WHERE activity.datname = current_database()
                      AND locks.relation = 'mcguildlink.account_links'::regclass
                      AND NOT locks.granted
                )",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("API did not reach the locked whitelist table");
    writer.commit().await.unwrap();

    assert_eq!(pending.await.unwrap().1, json!([{"uuid": FIRST, "name": "First"}]));
    assert_eq!(
        get(&app).await.1,
        json!([
            {"uuid": FIRST, "name": "First"},
            {"uuid": SECOND, "name": "Second"},
        ])
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn database_and_regeneration_failures_return_503_instead_of_cached_data(pool: PgPool) {
    let reader = reader(&pool).await;
    let app = router(reader.clone());
    assert_eq!(get(&app).await, (StatusCode::OK, json!([])));
    let discord = add_discord(&pool, 1).await;
    let minecraft = add_minecraft(&pool, FIRST, "First").await;
    link(&pool, discord, minecraft).await;
    sqlx::query("REVOKE SELECT ON mcguildlink.minecraft_accounts FROM platform_public_api_runtime")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.0, StatusCode::SERVICE_UNAVAILABLE);
    sqlx::query("GRANT SELECT ON mcguildlink.minecraft_accounts TO platform_public_api_runtime")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app).await.1, json!([{"uuid": FIRST, "name": "First"}]));
    reader.close().await;
    assert_eq!(get(&app).await.0, StatusCode::SERVICE_UNAVAILABLE);
}

#[sqlx::test(migrations = "../../migrations")]
async fn runtime_role_can_only_read_whitelist_sources(pool: PgPool) {
    let reader = reader(&pool).await;
    assert_eq!(get(&router(reader.clone())).await.0, StatusCode::OK);
    for forbidden in [
        "DELETE FROM mcguildlink.whitelist_revision",
        "INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 1)",
        "SELECT * FROM mcguildlink.audit_logs",
        "CREATE TABLE mcguildlink.forbidden (id integer)",
    ] {
        let error = sqlx::query(forbidden).execute(&reader).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"));
    }
}
