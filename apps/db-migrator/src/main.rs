use platform_database::DatabaseConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = DatabaseConfig::from_env()?.connect().await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    /// DB 変更のない次のリリースや、適用済み migration の再実行は履歴を変更しない。
    #[sqlx::test(migrations = false)]
    async fn repeated_migration_runs_preserve_applied_history(pool: PgPool) {
        let migrations = sqlx::migrate!("../../migrations");
        migrations.run(&pool).await.unwrap();
        let history_sql = "SELECT json_agg(m ORDER BY version)::text FROM public._sqlx_migrations m";
        let before: String = sqlx::query_scalar(history_sql).fetch_one(&pool).await.unwrap();
        for _ in 0..2 {
            migrations.run(&pool).await.unwrap();
            let after: String = sqlx::query_scalar(history_sql).fetch_one(&pool).await.unwrap();
            assert_eq!(before, after);
        }
    }
}
