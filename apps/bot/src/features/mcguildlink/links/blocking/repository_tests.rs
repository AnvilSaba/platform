use std::time::Duration;

use serde_json::json;
use sqlx::PgPool;

use super::repository::{BlockCause, BlockResult, DatabaseBlockRepository};
use crate::features::mcguildlink::GuildMembershipEventHandler;
use crate::features::mcguildlink::test_support::{self, FIRST, SECOND};

async fn counts(pool: &PgPool) -> (i64, i64, i64, i64) {
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.account_links")
        .fetch_one(pool)
        .await
        .unwrap();
    let codes: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.link_requests")
        .fetch_one(pool)
        .await
        .unwrap();
    let logs: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.audit_logs")
        .fetch_one(pool)
        .await
        .unwrap();
    let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.audit_outbox")
        .fetch_one(pool)
        .await
        .unwrap();
    (links, codes, logs, outbox)
}

#[sqlx::test(migrations = "../../migrations")]
async fn ban_blocks_the_full_many_to_many_component_and_unblock_releases_it(pool: PgPool) {
    test_support::seed_linked_accounts(&pool).await;
    let store = DatabaseBlockRepository::new(test_support::bot_pool(&pool).await);
    let handler = GuildMembershipEventHandler::new(&test_support::bot_pool(&pool).await);

    handler.on_ban(100, 200, 10, "alice").await.unwrap();
    assert_eq!(counts(&pool).await, (3, 1, 0, 0));
    handler.on_ban(100, 100, 10, "alice-now").await.unwrap();
    let groups = store.list().await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].root.user_id, "10");
    assert_eq!(
        groups[0]
            .discord
            .iter()
            .map(|account| account.user_id.as_str())
            .collect::<Vec<_>>(),
        ["10", "20"]
    );
    assert_eq!(
        groups[0]
            .minecraft
            .iter()
            .map(|account| account.uuid.to_string())
            .collect::<Vec<_>>(),
        [FIRST, SECOND]
    );
    assert_eq!(counts(&pool).await, (0, 0, 1, 1));
    assert_eq!(test_support::whitelist(&pool).await, json!([]));

    let log = sqlx::query_as::<_, (String, String, Option<String>, serde_json::Value, serde_json::Value)>(
        "SELECT event_type, actor_type, actor_discord_user_id::text, related_discord_accounts, related_minecraft_accounts \
         FROM mcguildlink.audit_logs",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (log.0.as_str(), log.1.as_str(), log.2),
        ("member_banned_blocked", "system", None)
    );
    assert_eq!(log.3.as_array().unwrap().len(), 2);
    assert_eq!(log.4.as_array().unwrap().len(), 2);

    // DB経由の紐付け・コード作成も拒否される。
    let blocked_discord_link =
        sqlx::query("INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 2)")
            .execute(&pool)
            .await;
    assert!(blocked_discord_link.is_err());
    let blocked_minecraft_link =
        sqlx::query("INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (30, 'carol')")
            .execute(&pool)
            .await
            .unwrap();
    assert_eq!(blocked_minecraft_link.rows_affected(), 1);
    assert!(sqlx::query("INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) SELECT id, 1 FROM mcguildlink.discord_accounts WHERE user_id = 30")
        .execute(&pool).await.is_err());
    assert!(
        sqlx::query("INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (2, 'BOBNEW')")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(matches!(
        store.block(20, "bob", BlockCause::Moderator).await.unwrap(),
        BlockResult::AlreadyBlocked
    ));
    assert_eq!(counts(&pool).await, (0, 0, 1, 1));

    let removed = store.unblock(20).await.unwrap().unwrap();
    assert_eq!(removed.root.user_id, "10");
    assert!(store.list().await.unwrap().is_empty());
    assert!(store.unblock(20).await.unwrap().is_none());
    assert_eq!(counts(&pool).await, (0, 0, 1, 1));
    sqlx::query("INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (2, 'BOBNEW')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) SELECT id, 1 FROM mcguildlink.discord_accounts WHERE user_id = 30")
        .execute(&pool).await.unwrap();
    assert_eq!(
        test_support::whitelist(&pool).await,
        json!([{"uuid": FIRST, "name": "First"}])
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn moderator_block_does_not_create_an_audit_log(pool: PgPool) {
    test_support::seed_linked_accounts(&pool).await;
    let store = DatabaseBlockRepository::new(test_support::bot_pool(&pool).await);
    assert!(matches!(
        store.block(10, "alice", BlockCause::Moderator).await.unwrap(),
        BlockResult::Blocked(_)
    ));
    assert_eq!(counts(&pool).await, (0, 0, 0, 0));
    store.unblock(10).await.unwrap();
    assert_eq!(counts(&pool).await, (0, 0, 0, 0));
}

#[sqlx::test(migrations = "../../migrations")]
async fn block_waits_for_inflight_link_and_code_before_collecting_accounts(pool: PgPool) {
    test_support::seed_linked_accounts(&pool).await;
    sqlx::query("INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (30, 'carol')")
        .execute(&pool)
        .await
        .unwrap();
    let mut pending = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) \
                 SELECT id, 1 FROM mcguildlink.discord_accounts WHERE user_id = 30",
    )
    .execute(&mut *pending)
    .await
    .unwrap();
    sqlx::query("INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (2, 'BOBCODE')")
        .execute(&mut *pending)
        .await
        .unwrap();

    let store = DatabaseBlockRepository::new(test_support::bot_pool(&pool).await);
    let mut blocking = tokio::spawn({
        let store = store.clone();
        async move { store.block(10, "alice", BlockCause::Moderator).await }
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut blocking)
            .await
            .is_err()
    );
    pending.commit().await.unwrap();
    assert!(matches!(blocking.await.unwrap().unwrap(), BlockResult::Blocked(_)));
    let groups = store.list().await.unwrap();
    assert_eq!(
        groups[0]
            .discord
            .iter()
            .map(|account| account.user_id.as_str())
            .collect::<Vec<_>>(),
        ["10", "20", "30"]
    );
    assert_eq!(counts(&pool).await, (0, 0, 0, 0));
    assert_eq!(test_support::whitelist(&pool).await, json!([]));
}

#[sqlx::test(migrations = "../../migrations")]
async fn failed_ban_audit_rolls_back_the_whole_block(pool: PgPool) {
    test_support::seed_linked_accounts(&pool).await;
    sqlx::raw_sql(
        "CREATE FUNCTION mcguildlink.reject_block_audit() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'audit unavailable'; END $$; \
         CREATE TRIGGER reject_block_audit BEFORE INSERT ON mcguildlink.audit_logs \
         FOR EACH ROW EXECUTE FUNCTION mcguildlink.reject_block_audit();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let handler = GuildMembershipEventHandler::new(&test_support::bot_pool(&pool).await);
    assert!(handler.on_ban(100, 100, 10, "alice").await.is_err());
    assert_eq!(counts(&pool).await, (3, 1, 0, 0));
    assert_eq!(
        test_support::whitelist(&pool).await,
        json!([
            {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
        ])
    );
}
