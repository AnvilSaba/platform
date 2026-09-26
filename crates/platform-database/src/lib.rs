use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("DATABASE_URL must be set to a valid PostgreSQL connection URL")]
    Environment(#[source] std::env::VarError),
    #[error("DATABASE_URL is not a valid PostgreSQL connection URL")]
    InvalidUrl(#[source] sqlx::Error),
    #[error("Failed to connect to PostgreSQL")]
    Connection(#[source] sqlx::Error),
}

/// 環境変数の接続設定を検証し、サービス共通の接続プールを作成する。
pub struct DatabaseConfig {
    options: PgConnectOptions,
}

impl DatabaseConfig {
    pub fn from_env() -> Result<Self, DatabaseError> {
        let url = std::env::var("DATABASE_URL").map_err(DatabaseError::Environment)?;
        let options = url.parse().map_err(DatabaseError::InvalidUrl)?;
        Ok(Self { options })
    }

    pub async fn connect(self) -> Result<PgPool, DatabaseError> {
        PgPoolOptions::new()
            .max_connections(10)
            .connect_with(self.options)
            .await
            .map_err(DatabaseError::Connection)
    }
}
