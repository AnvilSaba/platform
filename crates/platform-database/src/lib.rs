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

/// 対応済み履歴を照合し、required_through までの適用を必須とする。DDL は実行しない。
pub async fn check_migrations(
    pool: &PgPool,
    migrations: &sqlx::migrate::Migrator,
    required_through: i64,
) -> Result<(), MigrationHistoryError> {
    let applied = sqlx::query!("SELECT version, success, checksum FROM public._sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await?;
    if let Some(entry) = applied.iter().find(|entry| !entry.success) {
        return Err(MigrationHistoryError::Unsuccessful(entry.version));
    }
    for entry in &applied {
        if !migrations
            .iter()
            .any(|migration| migration.migration_type.is_up_migration() && migration.version == entry.version)
        {
            return Err(MigrationHistoryError::Unsupported(entry.version));
        }
    }
    for migration in migrations
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
    {
        let Some(entry) = applied.iter().find(|entry| entry.version == migration.version) else {
            if migration.version <= required_through {
                return Err(MigrationHistoryError::Missing(migration.version));
            }
            continue;
        };
        if entry.checksum.as_slice() != migration.checksum.as_ref() {
            return Err(MigrationHistoryError::ChecksumMismatch(migration.version));
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum MigrationHistoryError {
    #[error("Required migration {0} has not been applied")]
    Missing(i64),
    #[error("Migration {0} checksum does not match this application")]
    ChecksumMismatch(i64),
    #[error("Migration {0} has an unsuccessful execution record")]
    Unsuccessful(i64),
    #[error("Migration {0} is not supported by this application")]
    Unsupported(i64),
    #[error("Failed to read migration history")]
    Read(#[from] sqlx::Error),
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

    static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
    const REQUIRED_FIXTURE_VERSION: i64 = 20260926184758;

    #[sqlx::test(migrations = "tests/compatibility")]
    async fn known_compatible_addition_is_accepted_before_and_after_application(pool: PgPool) {
        let supported = sqlx::migrate!("tests/compatibility");
        supported.undo(&pool, 1).await.unwrap();
        check_migrations(&pool, &supported, 1).await.unwrap();
        assert!(matches!(
            check_migrations(&pool, &supported, 2).await,
            Err(MigrationHistoryError::Missing(2))
        ));
        supported.run(&pool).await.unwrap();
        check_migrations(&pool, &supported, 1).await.unwrap();
        check_migrations(&pool, &supported, 2).await.unwrap();
    }

    #[sqlx::test(migrations = false)]
    async fn absent_history_is_rejected_without_creating_it(pool: PgPool) {
        let result = check_migrations(&pool, &MIGRATIONS, REQUIRED_FIXTURE_VERSION).await;
        let Err(MigrationHistoryError::Read(error)) = result else {
            panic!("expected missing history error")
        };
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42P01"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn unknown_migration_is_rejected(pool: PgPool) {
        sqlx::query!(
            "INSERT INTO public._sqlx_migrations (version, description, success, checksum, execution_time)
             VALUES ($1, 'unknown future change', true, $2, 0)",
            20990101000000_i64,
            &[0_u8][..]
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            check_migrations(&pool, &MIGRATIONS, REQUIRED_FIXTURE_VERSION)
                .await
                .is_err()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn unsuccessful_migration_is_rejected(pool: PgPool) {
        sqlx::query!("UPDATE public._sqlx_migrations SET success = false")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            check_migrations(&pool, &MIGRATIONS, REQUIRED_FIXTURE_VERSION)
                .await
                .is_err()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn changed_migration_checksum_is_rejected(pool: PgPool) {
        sqlx::query!("UPDATE public._sqlx_migrations SET checksum = $1", &[0_u8][..])
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            check_migrations(&pool, &MIGRATIONS, REQUIRED_FIXTURE_VERSION)
                .await
                .is_err()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn missing_required_migration_is_rejected(pool: PgPool) {
        sqlx::query!("DELETE FROM public._sqlx_migrations")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            check_migrations(&pool, &MIGRATIONS, REQUIRED_FIXTURE_VERSION)
                .await
                .is_err()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn applied_history_is_accepted_without_schema_changes(pool: PgPool) {
        let reader = sqlx::postgres::PgPoolOptions::new()
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query!("SET default_transaction_read_only = on")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect_with((*pool.connect_options()).clone())
            .await
            .unwrap();
        check_migrations(&reader, &MIGRATIONS, REQUIRED_FIXTURE_VERSION)
            .await
            .unwrap();
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
