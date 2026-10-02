use super::resume_stopped;
use crate::app::{AppApplicationContext, AppError, BotDataExt};
use crate::features::mcguildlink::permissions::require_moderator;

/// 停止中の監査配送を再開します。ID省略時は停止中の全件が対象です。
#[poise::command(slash_command, ephemeral, guild_only, check = "require_moderator")]
pub async fn audit_retry(
    ctx: AppApplicationContext<'_>,
    #[description = "再開するイベントID（省略すると停止中の全件）"]
    #[min = 1]
    event_id: Option<i64>,
) -> Result<(), AppError> {
    ctx.defer_ephemeral().await?;
    let count = resume_stopped(&ctx.bot_data().database, event_id).await?;
    ctx.send(
        poise::CreateReply::default()
            .ephemeral(true)
            .content(format!("停止中の監査配送 {count} 件を再試行待ちに戻しました。")),
    )
    .await?;
    Ok(())
}
