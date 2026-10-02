use platform_database::DatabaseConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = DatabaseConfig::from_env()?.connect().await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    Ok(())
}
