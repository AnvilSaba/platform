use std::error::Error;

use platform_database::{DatabaseConfig, check_migrations};

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
const REQUIRED_MIGRATIONS: &[i64] = &[20260926184758, 20260928120000, 20260928180000];

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let pool = DatabaseConfig::from_env()?.connect().await?;
    check_migrations(&pool, &MIGRATIONS, REQUIRED_MIGRATIONS).await?;

    let address = std::env::var("PUBLIC_API_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    eprintln!("Public API listening on {address}");
    let shutdown_rx = platform_signal::shutdown_receiver()?;
    axum::serve(listener, public_api::router(pool))
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        })
        .await?;

    Ok(())
}
