use serde_json::json;
use sqlx::PgPool;

use super::{ports::MemberDepartureRepository, repository::DatabaseMemberDepartureRepository};
use crate::features::mcguildlink::test_support::{self, FIRST, SECOND};

async fn link_count(pool: &PgPool, user_id: u64) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM mcguildlink.account_links l \
         JOIN mcguildlink.discord_accounts d ON d.id = l.discord_account_id \
         WHERE d.user_id = $1::text::numeric",
    )
    .bind(user_id.to_string())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn leaving_removes_links_and_code_and_enqueues_audit(pool: PgPool) {
    test_support::seed_linked_accounts(&pool).await;
    let store = DatabaseMemberDepartureRepository::new(test_support::bot_pool(&pool).await);
    store.member_left(10, "alice-now").await.unwrap();
    assert_eq!(link_count(&pool, 10).await, 0);
    assert_eq!(link_count(&pool, 20).await, 1);
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
    assert_eq!(
        test_support::whitelist(&pool).await,
        json!([{"uuid": FIRST, "name": "First"}])
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn audit_failure_rolls_back_leave(pool: PgPool) {
    test_support::seed_linked_accounts(&pool).await;
    sqlx::raw_sql(
        "CREATE FUNCTION mcguildlink.reject_test_audit() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'audit unavailable'; END $$;
        CREATE TRIGGER reject_test_audit BEFORE INSERT ON mcguildlink.audit_logs
        FOR EACH ROW EXECUTE FUNCTION mcguildlink.reject_test_audit();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let store = DatabaseMemberDepartureRepository::new(test_support::bot_pool(&pool).await);
    assert!(store.member_left(10, "alice").await.is_err());
    assert_eq!(link_count(&pool, 10).await, 2);
    let code = sqlx::query_scalar!("SELECT code FROM mcguildlink.link_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(code, "ALICECODE");
    assert_eq!(
        test_support::whitelist(&pool).await,
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
    let store = DatabaseMemberDepartureRepository::new(test_support::bot_pool(&pool).await);
    store.member_left(10, "alice").await.unwrap();
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
