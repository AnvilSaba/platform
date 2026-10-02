use super::{model::Link, ports::AccountLinksRepository, queries};
use crate::app::AppError;
use serenity::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct DatabaseAccountLinksRepository {
    pool: PgPool,
}

impl DatabaseAccountLinksRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn list(&self, user_id: Option<u64>, uuid: Option<Uuid>) -> Result<Vec<Link>, AppError> {
        Ok(queries::list(&self.pool, user_id, uuid).await?)
    }
}

#[async_trait]
impl AccountLinksRepository for DatabaseAccountLinksRepository {
    async fn by_discord(&self, user_id: u64) -> Result<Vec<Link>, AppError> {
        self.list(Some(user_id), None).await
    }

    async fn by_minecraft(&self, uuid: Uuid) -> Result<Vec<Link>, AppError> {
        self.list(None, Some(uuid)).await
    }

    async fn all(&self) -> Result<Vec<Link>, AppError> {
        self.list(None, None).await
    }

    async fn unlink(&self, user_id: u64, uuid: Uuid) -> Result<bool, AppError> {
        Ok(queries::unlink(&self.pool, user_id, uuid).await?)
    }
}
