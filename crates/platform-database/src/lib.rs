use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use thiserror::Error;

/// 既存のプロバイダーを尊重し、未登録の場合だけ既定値を設定する。
fn install_crypto_provider_if_absent() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        // 確認後に別の処理が先に登録した場合も、そのプロバイダーを使う。
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_crypto_provider_is_preserved() {
        install_crypto_provider_if_absent();
        let provider = rustls::crypto::CryptoProvider::get_default().unwrap().clone();
        install_crypto_provider_if_absent();
        assert!(std::sync::Arc::ptr_eq(
            &provider,
            rustls::crypto::CryptoProvider::get_default().unwrap()
        ));
        let _ = rustls::ClientConfig::builder();
    }
}

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
        install_crypto_provider_if_absent();
        PgPoolOptions::new()
            .max_connections(10)
            .connect_with(self.options)
            .await
            .map_err(DatabaseError::Connection)
    }
}
