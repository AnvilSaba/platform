use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    cache: Arc<Mutex<Option<CachedWhitelist>>>,
}

struct CachedWhitelist {
    revision: i64,
    json: String,
}

#[derive(Serialize)]
struct WhitelistEntry {
    uuid: Uuid,
    name: String,
}

/// 各プロセスが自分の更新番号と JSON を保持する公開ルーター。
pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/whitelist.json", get(whitelist))
        .with_state(AppState {
            pool,
            cache: Arc::new(Mutex::new(None)),
        })
}

async fn whitelist(State(state): State<AppState>) -> Response {
    match current_json(&state).await {
        Ok(json) => ([(header::CONTENT_TYPE, "application/json")], json).into_response(),
        Err(error) => {
            eprintln!("Failed to read whitelist: {error}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

async fn current_json(state: &AppState) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // このロックは同一プロセス内の再生成を直列化する。各要求は必ず DB の番号を読む。
    let mut cache = state.cache.lock().await;
    let revision = sqlx::query_scalar!("SELECT revision FROM mcguildlink.whitelist_revision WHERE singleton",)
        .fetch_one(&state.pool)
        .await?;
    if let Some(cached) = cache.as_ref()
        && cached.revision == revision
    {
        return Ok(cached.json.clone());
    }

    // 番号と一覧は REPEATABLE READ の同じスナップショットから読む。
    let mut transaction = state.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;
    let snapshot_revision = sqlx::query_scalar!("SELECT revision FROM mcguildlink.whitelist_revision WHERE singleton",)
        .fetch_one(&mut *transaction)
        .await?;
    let entries = sqlx::query_as!(
        WhitelistEntry,
        "SELECT DISTINCT m.uuid AS \"uuid!\", m.last_known_name AS \"name!\"
         FROM mcguildlink.account_links AS links
         JOIN mcguildlink.minecraft_accounts AS m ON m.id = links.minecraft_account_id
         WHERE NOT EXISTS (
             SELECT 1 FROM mcguildlink.blocked_discord_accounts AS blocked
             WHERE blocked.discord_account_id = links.discord_account_id
         ) AND NOT EXISTS (
             SELECT 1 FROM mcguildlink.blocked_minecraft_accounts AS blocked
             WHERE blocked.minecraft_account_id = m.id
         )
         ORDER BY m.uuid",
    )
    .fetch_all(&mut *transaction)
    .await?;
    let json = serde_json::to_string(&entries)?;
    transaction.commit().await?;
    *cache = Some(CachedWhitelist {
        revision: snapshot_revision,
        json: json.clone(),
    });
    Ok(json)
}
