use super::{
    presentation::{Scope, load, page, save_snapshot},
    repository::DatabaseAccountLinksRepository,
};
use crate::{
    app::{AppApplicationContext, AppError, BotDataExt, BotError},
    utils::create_safe_allowed_mentions,
};
use poise::CreateReply;
use serenity::all::User;
use uuid::Uuid;

async fn moderator(ctx: AppApplicationContext<'_>) -> Result<bool, AppError> {
    let config = ctx.app_config().await;
    Ok(ctx.guild_id() == Some(config.mcguildlink.guild_id)
        && ctx
            .interaction
            .member
            .as_ref()
            .is_some_and(|member| member.roles.contains(&config.mcguildlink.moderator_role_id)))
}

async fn send_command_page(ctx: AppApplicationContext<'_>, scope: Scope) -> Result<(), AppError> {
    if !moderator(ctx).await? {
        return Err(BotError::HasNoRole.into());
    }
    ctx.defer_ephemeral().await?;
    let data = ctx.bot_data();
    let repository = DatabaseAccountLinksRepository::new(data.database.clone());
    let links = load(&repository, scope).await?;
    let snapshot = save_snapshot(ctx.interaction.id.get(), ctx.author().id.get(), scope, links);
    let (content, components) = page(ctx.interaction.id.get(), &snapshot, 0);
    ctx.send(
        CreateReply::default()
            .content(content)
            .components(components)
            .ephemeral(true)
            .allowed_mentions(create_safe_allowed_mentions()),
    )
    .await?;
    Ok(())
}

/// 紐付け済みアカウントの一覧を表示します。
#[poise::command(
    slash_command,
    ephemeral,
    guild_only,
    subcommands("links_discord", "links_minecraft", "links_all")
)]
pub async fn links(_: AppApplicationContext<'_>) -> Result<(), AppError> {
    Ok(())
}

/// 指定した Discordアカウントの紐付け一覧を表示します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "discord")]
pub async fn links_discord(
    ctx: AppApplicationContext<'_>,
    #[description = "一覧表示する Discordユーザー"] user: User,
) -> Result<(), AppError> {
    send_command_page(ctx, Scope::Discord(user.id.get())).await
}

/// 指定した Minecraft UUID の紐付け一覧を表示します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "minecraft")]
pub async fn links_minecraft(
    ctx: AppApplicationContext<'_>,
    #[description = "一覧表示する Minecraft UUID"] uuid: String,
) -> Result<(), AppError> {
    if !moderator(ctx).await? {
        return Err(BotError::HasNoRole.into());
    }
    let Ok(uuid) = Uuid::parse_str(&uuid) else {
        ctx.say("Minecraft UUID は `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` 形式で指定してください。")
            .await?;
        return Ok(());
    };
    send_command_page(ctx, Scope::Minecraft(uuid)).await
}

/// 全ての紐付け一覧を表示します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "all")]
pub async fn links_all(ctx: AppApplicationContext<'_>) -> Result<(), AppError> {
    send_command_page(ctx, Scope::All).await
}
