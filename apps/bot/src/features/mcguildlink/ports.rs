use super::types::{DiscordUserId, LinkCode};
use crate::app::AppError;
use serenity::async_trait;

#[derive(Debug, PartialEq, Eq)]
pub enum LinkCodeResult {
    Code(LinkCode),
    Blocked,
}

#[async_trait]
pub trait LinkCodes: Send + Sync {
    async fn issue(&self, user_id: DiscordUserId, username: &str) -> Result<LinkCodeResult, AppError>;
}

pub trait LinkCodeGenerator: Send + Sync {
    fn generate(&self) -> LinkCode;
}
