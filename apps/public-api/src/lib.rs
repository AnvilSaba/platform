mod whitelist_store;

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::Mutex;

use whitelist_store::{WhitelistVersion, snapshot, version};

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    cache: Arc<Mutex<Option<CachedWhitelist>>>,
}

#[derive(Clone)]
struct CachedWhitelist {
    version: WhitelistVersion,
    json: String,
    etag: String,
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

async fn whitelist(State(state): State<AppState>, request_headers: HeaderMap) -> Response {
    match current_whitelist(&state).await {
        Ok(cached) => respond_whitelist(cached, &request_headers),
        Err(error) => {
            eprintln!("Failed to read whitelist: {error}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

async fn current_whitelist(state: &AppState) -> Result<CachedWhitelist, Box<dyn std::error::Error + Send + Sync>> {
    // このロックは同一プロセス内の再生成を直列化する。各要求は必ず DB の番号を読む。
    let mut cache = state.cache.lock().await;
    let db_version = version(&state.pool).await?;
    if let Some(cached) = cache.as_ref()
        && cached.version.revision == db_version.revision
    {
        return Ok(cached.clone());
    }

    let snapshot = snapshot(&state.pool).await?;
    let json = serde_json::to_string(&snapshot.entries)?;
    let etag = format!("\"{:x}\"", Sha256::digest(json.as_bytes()));
    let refreshed = CachedWhitelist {
        version: snapshot.version,
        json,
        etag,
    };
    *cache = Some(refreshed.clone());
    Ok(refreshed)
}

fn respond_whitelist(cached: CachedWhitelist, request_headers: &HeaderMap) -> Response {
    let modified_at = UNIX_EPOCH + Duration::from_secs(cached.version.last_modified_at.timestamp() as u64);
    let last_modified = httpdate::fmt_http_date(modified_at);

    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(
        header::LAST_MODIFIED,
        HeaderValue::from_str(&last_modified).expect("HTTP date"),
    );
    headers.insert(header::ETAG, HeaderValue::from_str(&cached.etag).expect("SHA-256 ETag"));

    if is_not_modified(request_headers, &cached, modified_at) {
        (StatusCode::NOT_MODIFIED, headers).into_response()
    } else {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        (headers, cached.json).into_response()
    }
}

fn is_not_modified(request_headers: &HeaderMap, cached: &CachedWhitelist, modified_at: SystemTime) -> bool {
    if let Some(value) = request_headers.get(header::IF_NONE_MATCH) {
        return value.to_str().is_ok_and(|value| {
            value.split(',').any(|candidate| {
                let candidate = candidate.trim();
                candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == cached.etag
            })
        });
    }

    cached.version.if_modified_since_safe
        && request_headers.get(header::IF_MODIFIED_SINCE).is_some_and(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| httpdate::parse_http_date(value).ok())
                .is_some_and(|date| date >= modified_at)
        })
}
