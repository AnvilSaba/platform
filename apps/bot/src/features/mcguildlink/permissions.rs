use crate::app::{AppContext, AppError, BotDataExt, BotError};

pub(super) async fn require_moderator(ctx: AppContext<'_>) -> Result<bool, AppError> {
    let config = ctx.app_config().await;
    if ctx.guild_id() == Some(config.mcguildlink.guild_id)
        && ctx
            .author_member()
            .await
            .is_some_and(|member| member.roles.contains(&config.mcguildlink.moderator_role_id))
    {
        Ok(true)
    } else {
        Err(BotError::HasNoRole.into())
    }
}
