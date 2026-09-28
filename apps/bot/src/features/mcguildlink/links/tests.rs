use super::{
    event_handler::member_left_for_guild,
    presentation::{Scope, get_snapshot, page, save_snapshot},
    store::LinkManagement,
};
use crate::features::mcguildlink::test_support;
use serenity::all::GuildId;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "../../migrations")]
async fn other_guild_leave_keeps_links_code_and_audit_unchanged(pool: PgPool) {
    sqlx::raw_sql(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice');
            INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name)
                VALUES ('00000000-0000-0000-0000-000000000001', 'First');
            INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 1);
            INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'ALICECODE');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let store = LinkManagement::new(test_support::bot_pool(&pool).await);
    member_left_for_guild(&store, GuildId::new(100), GuildId::new(200), 10, "alice")
        .await
        .unwrap();
    assert_eq!(store.by_discord(10).await.unwrap().len(), 1);
    let codes = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM mcguildlink.link_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    let audit = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM mcguildlink.audit_logs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((codes, audit), (1, 0));
}

#[sqlx::test(migrations = "../../migrations")]
async fn page_uses_initial_snapshot_after_a_link_changes(pool: PgPool) {
    sqlx::raw_sql(
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice');
            INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name)
                VALUES ('00000000-0000-0000-0000-000000000001', 'First');
            INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 1);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let store = LinkManagement::new(test_support::bot_pool(&pool).await);
    save_snapshot(1000, 10, Scope::User(10), store.by_discord(10).await.unwrap());
    assert!(
        store
            .unlink(10, Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
            .await
            .unwrap()
    );
    assert!(store.by_discord(10).await.unwrap().is_empty());
    let snapshot = get_snapshot(1000).unwrap();
    assert!(page(1000, &snapshot, 0).0.contains("First"));
    assert_eq!(snapshot.links.len(), 1);
}
