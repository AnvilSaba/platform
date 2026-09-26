use super::types::{DiscordUserId, LinkCode};
use crate::app::AppError;
use serenity::async_trait;

#[derive(Debug, PartialEq, Eq)]
pub enum LinkCodeResult {
    Code(LinkCode),
    Blocked,
}

/// MCGuildLink の永続化境界。利用者を登録・更新し、行ロックを持つセッションを開始する。
#[async_trait]
pub trait McGuildLinkRepository: Send + Sync {
    type Session: McGuildLinkSession;

    async fn begin(&self, user_id: DiscordUserId, username: &str) -> Result<Self::Session, AppError>;
}

/// 発行処理全体で一つのトランザクションを保持する。未コミットで破棄した場合はロールバックする。
#[async_trait]
pub trait McGuildLinkSession: Send {
    async fn is_blocked(&mut self) -> Result<bool, AppError>;
    async fn unused_code(&mut self) -> Result<Option<LinkCode>, AppError>;
    /// 保存できた場合は true、コードの衝突時は false を返す。
    async fn reserve_code(&mut self, code: &LinkCode) -> Result<bool, AppError>;
    async fn commit(self) -> Result<(), AppError>;
}

pub trait LinkCodeGenerator: Send + Sync {
    fn generate(&self) -> LinkCode;
}
