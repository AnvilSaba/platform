use serenity::async_trait;

use super::types::{DiscordUserId, LinkCode};
use crate::app::AppError;

#[derive(Debug, PartialEq, Eq)]
pub enum LinkCodeResult {
    Code(LinkCode),
    Blocked,
}

/// コード発行ユースケースの永続化ポート。行ロックを保持するセッションを提供する。
#[async_trait]
pub trait LinkingRepository: Send + Sync {
    type CodeIssuance: CodeIssuanceSession;

    /// コード発行用に利用者を登録・更新し、その行ロックを保持するセッションを開始する。
    async fn begin_code_issuance(&self, user_id: DiscordUserId, username: &str)
    -> Result<Self::CodeIssuance, AppError>;
}

/// コード発行専用の操作と、一連の発行処理全体のトランザクションを保持する。未コミットで破棄した場合はロールバックする。
#[async_trait]
pub trait CodeIssuanceSession: Send {
    async fn is_blocked(&mut self) -> Result<bool, AppError>;
    async fn unused_code(&mut self) -> Result<Option<LinkCode>, AppError>;
    /// 保存できた場合は true、コードの衝突時は false を返す。
    async fn reserve_code(&mut self, code: &LinkCode) -> Result<bool, AppError>;
    async fn commit(self) -> Result<(), AppError>;
}

pub trait LinkCodeGenerator: Send + Sync {
    fn generate(&self) -> LinkCode;
}
