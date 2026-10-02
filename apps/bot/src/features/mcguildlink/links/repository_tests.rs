use super::ports::AccountLinksRepository;
use super::repository::DatabaseAccountLinksRepository;
use crate::features::mcguildlink::test_support::{self, FIRST, SECOND};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn lists_and_unlinks_only_the_selected_relationship(pool: PgPool) {
    test_support::seed_linked_accounts(&pool).await;
    let store = DatabaseAccountLinksRepository::new(test_support::bot_pool(&pool).await);
    assert_eq!(store.by_discord(10).await.unwrap().len(), 2);
    assert_eq!(store.by_minecraft(FIRST.parse().unwrap()).await.unwrap().len(), 2);
    assert_eq!(store.all().await.unwrap().len(), 3);
    assert_eq!(
        test_support::whitelist(&pool).await,
        json!([
            {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
        ])
    );

    assert!(store.unlink(10, FIRST.parse().unwrap()).await.unwrap());
    assert!(!store.unlink(10, FIRST.parse().unwrap()).await.unwrap());
    assert_eq!(store.by_discord(10).await.unwrap().len(), 1);
    assert_eq!(store.by_minecraft(FIRST.parse().unwrap()).await.unwrap().len(), 1);
    assert_eq!(
        test_support::whitelist(&pool).await,
        json!([
            {"uuid": FIRST, "name": "First"}, {"uuid": SECOND, "name": "Second"}
        ])
    );
    assert!(store.unlink(20, FIRST.parse().unwrap()).await.unwrap());
    assert_eq!(
        test_support::whitelist(&pool).await,
        json!([{"uuid": SECOND, "name": "Second"}])
    );
    let audit_count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM mcguildlink.audit_logs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit_count, 0);
}
