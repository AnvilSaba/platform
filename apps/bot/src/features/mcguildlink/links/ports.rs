use super::model::Link;
use crate::app::AppError;
use serenity::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait AccountLinksRepository: Send + Sync {
    async fn by_discord(&self, user_id: u64) -> Result<Vec<Link>, AppError>;
    async fn by_minecraft(&self, uuid: Uuid) -> Result<Vec<Link>, AppError>;
    async fn all(&self) -> Result<Vec<Link>, AppError>;
    async fn unlink(&self, user_id: u64, uuid: Uuid) -> Result<bool, AppError>;
}
