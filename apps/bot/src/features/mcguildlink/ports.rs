use crate::app::AppError;
use serenity::async_trait;

#[derive(Debug, PartialEq, Eq)]
pub enum LinkCodeResult {
    Code(String),
    Blocked,
}

#[async_trait]
pub trait LinkCodes: Send + Sync {
    async fn issue(&self, user_id: u64, username: &str) -> Result<LinkCodeResult, AppError>;
}

/// Discord の通信アダプターとテスト用アダプターで差し替える応答先。
#[async_trait]
pub trait LinkReply: Send + Sync {
    async fn defer_ephemeral(&self) -> Result<(), AppError>;
    async fn complete(&self, content: String) -> Result<(), AppError>;
}
