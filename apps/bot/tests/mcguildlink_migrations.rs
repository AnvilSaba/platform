//! マイグレーションが定める DB の制約・権限を直接検証する。Bot のアダプター実装には依存しない。

use sqlx::PgPool;

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// DB の INSERT 制限を確認する。未ブロック時は成功し、ブロック済みのコード発行と双方の紐付けは拒否される。
#[sqlx::test(migrations = "../../migrations")]
async fn database_rejects_inserts_for_blocked_accounts(pool: PgPool) {
    sqlx::raw_sql("INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (1, 'blocked'), (2, 'allowed');
            INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ('00000000-0000-0000-0000-000000000001', 'blocked'), ('00000000-0000-0000-0000-000000000002', 'allowed');
            INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (2, 'allowed');
            INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (2, 2);
            INSERT INTO mcguildlink.block_groups (root_discord_account_id) VALUES (1);
            INSERT INTO mcguildlink.blocked_discord_accounts (discord_account_id, block_group_id) VALUES (1, 1);
            INSERT INTO mcguildlink.blocked_minecraft_accounts (minecraft_account_id, block_group_id) VALUES (1, 1);")
            .execute(&pool).await.unwrap();
    for rejected in [
        "INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'blocked')",
        "INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 2)",
        "INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (2, 1)",
    ] {
        let error = sqlx::query(rejected).execute(&pool).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
    }
}

async fn bot_pool(pool: &PgPool) -> PgPool {
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

/// Bot の権限を確認する。コード保存・読み取り・名前更新・履歴照合は許可し、DDL や対象外操作は拒否する。
#[sqlx::test(migrations = "../../migrations")]
async fn bot_role_allows_code_storage_but_rejects_schema_and_unrelated_changes(pool: PgPool) {
    let bot = bot_pool(&pool).await;
    let required: Vec<_> = MIGRATIONS
        .iter()
        .filter(|m| m.migration_type.is_up_migration())
        .map(|m| m.version)
        .collect();
    platform_database::check_migrations(&bot, &MIGRATIONS, &required)
        .await
        .unwrap();
    for allowed in [
        "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (321, 'restricted')",
        "INSERT INTO mcguildlink.link_requests (discord_account_id, code) SELECT id, 'AC234679' FROM mcguildlink.discord_accounts WHERE user_id = 321",
        "UPDATE mcguildlink.discord_accounts SET last_known_username = 'updated' WHERE user_id = 321",
    ] {
        assert_eq!(sqlx::query(allowed).execute(&bot).await.unwrap().rows_affected(), 1);
    }
    let code: String = sqlx::query_scalar("SELECT code FROM mcguildlink.link_requests")
        .fetch_one(&bot)
        .await
        .unwrap();
    assert_eq!(code, "AC234679");
    for forbidden in [
        "CREATE TABLE mcguildlink.forbidden (id integer)",
        "DELETE FROM mcguildlink.link_requests",
        "DELETE FROM public._sqlx_migrations",
        "INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name) VALUES ('00000000-0000-0000-0000-000000000001', 'player')",
        "SELECT * FROM mcguildlink.block_groups",
    ] {
        let error = sqlx::query(forbidden).execute(&bot).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"));
    }
}
