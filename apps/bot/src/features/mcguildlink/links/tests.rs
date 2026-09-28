use super::{
    ports::AccountLinksRepository,
    presentation::{Scope, get_snapshot, page, save_snapshot},
};
use crate::features::mcguildlink::{repository::DatabaseMcGuildLinkRepository, test_support};
use sqlx::PgPool;
use uuid::Uuid;

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
    let store = DatabaseMcGuildLinkRepository::new(test_support::bot_pool(&pool).await);
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
