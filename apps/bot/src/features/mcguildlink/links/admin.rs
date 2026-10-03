use poise::CreateReply;
use serenity::all::{MessageFlags, User};
use uuid::Uuid;

use super::{
    presentation::{ListPage, Scope, load, page, save_snapshot},
    repository::DatabaseAccountLinksRepository,
};
use crate::features::mcguildlink::permissions::require_moderator;
use crate::{
    app::{AppApplicationContext, AppError, BotDataExt},
    utils::create_safe_allowed_mentions,
};

async fn send_command_page(ctx: AppApplicationContext<'_>, scope: Scope) -> Result<(), AppError> {
    ctx.defer_ephemeral().await?;
    let data = ctx.bot_data();
    let repository = DatabaseAccountLinksRepository::new(data.database.clone());
    let links = load(&repository, scope).await?;
    let snapshot = save_snapshot(ctx.interaction.id.get(), ctx.author().id.get(), scope, links);
    let reply = CreateReply::default()
        .ephemeral(true)
        .allowed_mentions(create_safe_allowed_mentions());
    let reply = match page(ctx.interaction.id.get(), &snapshot, 0) {
        ListPage::Empty(content) => reply.content(content),
        ListPage::Components(components) => reply.flags(MessageFlags::IS_COMPONENTS_V2).components(components),
    };
    ctx.send(reply).await?;
    Ok(())
}

/// 紐付け済みアカウントの一覧を表示します。
#[poise::command(
    slash_command,
    ephemeral,
    guild_only,
    check = "require_moderator",
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
