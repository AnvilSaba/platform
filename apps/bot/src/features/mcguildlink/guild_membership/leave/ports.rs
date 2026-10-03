use serenity::async_trait;

use crate::app::AppError;

/// 退出に伴う紐付け・未使用コードの削除と監査記録を一括で確定する。
#[async_trait]
pub trait MemberDepartureRepository: Send + Sync {
    async fn member_left(&self, user_id: u64, username: &str) -> Result<(), AppError>;
}
