use poise::CreateReply;
use serenity::{
    all::{ButtonStyle, CreateActionRow, CreateButton, MessageFlags, ReactionType, SeparatorSpacingSize},
    builder::{
        CreateComponent, CreateContainer, CreateContainerComponent, CreateMessage, CreateSeparator, CreateTextDisplay,
    },
};

use super::permissions::require_moderator;
use super::{linking::START_LINK_BUTTON_ID, links::LIST_LINK_BUTTON_ID};
use crate::{
    app::{AppApplicationContext, AppError},
    utils::{create_safe_allowed_mentions, create_safe_message},
};

fn link_panel_message() -> CreateMessage<'static> {
    let buttons = CreateActionRow::buttons(vec![
        CreateButton::new(START_LINK_BUTTON_ID)
            .label("MCアカウントと紐付ける")
            .emoji(ReactionType::Unicode('🔗'.into()))
            .style(ButtonStyle::Primary),
        CreateButton::new(LIST_LINK_BUTTON_ID)
            .label("紐付けられたアカウントを確認する")
            .emoji(ReactionType::Unicode('📋'.into()))
            .style(ButtonStyle::Secondary),
    ]);
    create_safe_message()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "Minecraftアカウントと Discordアカウントを紐付けます。\n「MCアカウントと紐付ける」ボタンを押して、指示に従ってください。\n「紐付けられたアカウントを確認する」ボタンを押すと、現在紐付けられているアカウントの一覧を確認できます。",
            )),
            CreateContainerComponent::Separator(CreateSeparator::new().spacing(SeparatorSpacingSize::Large)),
            CreateContainerComponent::ActionRow(buttons),
        ]))])
}

/// 紐付けを開始するためのパネルを送信します。
#[poise::command(slash_command, ephemeral, guild_only, check = "require_moderator")]
pub async fn create_panel(ctx: AppApplicationContext<'_>) -> Result<(), AppError> {
    ctx.defer_ephemeral().await?;

    ctx.channel_id().send_message(ctx.http(), link_panel_message()).await?;

    ctx.send(
        CreateReply::default()
            .allowed_mentions(create_safe_allowed_mentions())
            .content("パネルを作成しました！"),
    )
    .await?;

    Ok(())
}
