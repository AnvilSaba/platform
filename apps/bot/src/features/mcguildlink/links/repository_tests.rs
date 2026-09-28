use super::ports::AccountLinksRepository;
use crate::features::mcguildlink::repository::DatabaseMcGuildLinkRepository;
use crate::features::mcguildlink::test_support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const FIRST: &str = "00000000-0000-0000-0000-000000000001";
const SECOND: &str = "00000000-0000-0000-0000-000000000002";

async fn seed(pool: &PgPool) {
    sqlx::raw_sql(&format!(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice'), (20, 'bob');
         INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ('{FIRST}', 'First'), ('{SECOND}', 'Second');
         INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id)
             VALUES (1, 1), (1, 2), (2, 1);
         INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'ALICECODE');"
    ))
    .execute(pool).await.unwrap();
}

async fn whitelist(pool: &PgPool) -> Value {
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

#[sqlx::test(migrations = "../../migrations")]
async fn lists_and_unlinks_only_the_selected_relationship(pool: PgPool) {
    seed(&pool).await;
    let store = DatabaseMcGuildLinkRepository::new(test_support::bot_pool(&pool).await);
    assert_eq!(store.by_discord(10).await.unwrap().len(), 2);
    assert_eq!(store.by_minecraft(FIRST.parse().unwrap()).await.unwrap().len(), 2);
    assert_eq!(store.all().await.unwrap().len(), 3);
    assert_eq!(
        whitelist(&pool).await,
        json!([
            {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
        ])
    );

    assert!(store.unlink(10, FIRST.parse().unwrap()).await.unwrap());
    assert!(!store.unlink(10, FIRST.parse().unwrap()).await.unwrap());
    assert_eq!(store.by_discord(10).await.unwrap().len(), 1);
    assert_eq!(store.by_minecraft(FIRST.parse().unwrap()).await.unwrap().len(), 1);
    assert_eq!(
        whitelist(&pool).await,
        json!([
            {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
        ])
    );
    assert!(store.unlink(20, FIRST.parse().unwrap()).await.unwrap());
    assert_eq!(whitelist(&pool).await, json!([{"uuid": SECOND, "name": "Second"}]));
    let audit_count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM mcguildlink.audit_logs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit_count, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn leaving_removes_links_and_code_and_enqueues_audit(pool: PgPool) {
    seed(&pool).await;
    let store = DatabaseMcGuildLinkRepository::new(test_support::bot_pool(&pool).await);
    let removed = store.member_left(10, "alice-now").await.unwrap();
    assert_eq!(removed.len(), 2);
    assert!(store.by_discord(10).await.unwrap().is_empty());
    assert_eq!(store.by_discord(20).await.unwrap().len(), 1);
    let code_count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM mcguildlink.link_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(code_count, 0);
    let audit = sqlx::query!(
        "SELECT a.event_type AS \"event_type!\", a.actor_discord_user_id::text AS \"actor_discord_user_id!\", a.target_minecraft_name AS \"target_minecraft_name!\"
         FROM mcguildlink.audit_logs a JOIN mcguildlink.audit_outbox o ON o.log_id = a.id
         ORDER BY a.target_minecraft_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        audit
            .into_iter()
            .map(|row| (row.event_type, row.actor_discord_user_id, row.target_minecraft_name))
            .collect::<Vec<_>>(),
        vec![
            ("member_leave_unlinked".into(), "10".into(), "First".into()),
            ("member_leave_unlinked".into(), "10".into(), "Second".into()),
        ]
    );
    assert_eq!(whitelist(&pool).await, json!([{"uuid": FIRST, "name": "First"}]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn audit_failure_rolls_back_leave(pool: PgPool) {
    seed(&pool).await;
    sqlx::raw_sql(
        "CREATE FUNCTION mcguildlink.reject_test_audit() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'audit unavailable'; END $$;
        CREATE TRIGGER reject_test_audit BEFORE INSERT ON mcguildlink.audit_logs
        FOR EACH ROW EXECUTE FUNCTION mcguildlink.reject_test_audit();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let store = DatabaseMcGuildLinkRepository::new(test_support::bot_pool(&pool).await);
    assert!(store.member_left(10, "alice").await.is_err());
    assert_eq!(store.by_discord(10).await.unwrap().len(), 2);
    let code = sqlx::query_scalar!("SELECT code FROM mcguildlink.link_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(code, "ALICECODE");
    assert_eq!(
        whitelist(&pool).await,
        json!([
            {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
        ])
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn leaving_with_only_unused_code_removes_it_without_audit(pool: PgPool) {
    sqlx::raw_sql(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice');
        INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'ALICECODE');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let store = DatabaseMcGuildLinkRepository::new(test_support::bot_pool(&pool).await);
    assert!(store.member_left(10, "alice").await.unwrap().is_empty());
    let code_count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM mcguildlink.link_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    let audit_count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM mcguildlink.audit_logs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((code_count, audit_count), (0, 0));
}
