use crate::app::{AppApplicationContext, AppError, BotDataExt, BotError};

pub(super) async fn require_moderator(ctx: AppApplicationContext<'_>) -> Result<(), AppError> {
    let config = ctx.app_config().await;
    if ctx.guild_id() == Some(config.mcguildlink.guild_id)
        && ctx
            .interaction
            .member
            .as_ref()
            .is_some_and(|member| member.roles.contains(&config.mcguildlink.moderator_role_id))
    {
        Ok(())
    } else {
        Err(BotError::HasNoRole.into())
    }
}
