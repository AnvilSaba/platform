use super::{
    ports::LinkCodeResult, repository::DatabaseMcGuildLinkRepository, service::LinkCodes, types::DiscordUserId,
};
use crate::{
    app::{AppApplicationContext, AppError, BotDataExt, BotError},
    utils::{create_safe_allowed_mentions, create_safe_message},
};
use bot_macros::event_handler;
use poise::CreateReply;
use serenity::{
    all::{
        ButtonStyle, ComponentInteractionDataKind, Context, CreateActionRow, CreateButton, EditInteractionResponse,
        FullEvent, Interaction, MessageFlags, ReactionType, SeparatorSpacingSize,
    },
    builder::{
        CreateComponent, CreateContainer, CreateContainerComponent, CreateMessage, CreateSeparator, CreateTextDisplay,
    },
};

const START_LINK_BUTTON_ID: &str = "start_link_button";
pub(super) const LIST_LINK_BUTTON_ID: &str = "list_link_button";

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

#[event_handler]
pub async fn handle_link_event(ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
    let FullEvent::InteractionCreate {
        interaction: Interaction::Component(interaction),
        ..
    } = event
    else {
        return Ok(());
    };

    if interaction.data.custom_id != START_LINK_BUTTON_ID
        || !matches!(interaction.data.kind, ComponentInteractionDataKind::Button)
    {
        return Ok(());
    }

    let config = ctx.app_config().await;
    let config = &config.mcguildlink;
    if interaction.guild_id != Some(config.guild_id) {
        return Ok(());
    }
    let data = ctx.bot_data();

    interaction.defer_ephemeral(&ctx.http).await?;

    let codes = LinkCodes::new(DatabaseMcGuildLinkRepository::new(data.database.clone()));
    let result = codes
        .issue(DiscordUserId::new(interaction.user.id.get()), &interaction.user.name)
        .await;

    let content = match &result {
        Ok(LinkCodeResult::Code(code)) => format!(
            "Minecraft 26.3 で以下のサーバーに接続し、表示される入力欄にコードを入力してください。\n\nサーバーアドレス:\n```\n{}\n```\nコード:\n```\n{code}\n```",
            config.display_server_address
        ),
        Ok(LinkCodeResult::Blocked) => {
            "この Discordアカウントはブロックされているため、紐付けを開始できません。".into()
        }
        Err(_) => "コードを取得できませんでした。時間をおいて再度お試しください。".into(),
    };

    interaction
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new()
                .content(content)
                .allowed_mentions(create_safe_allowed_mentions()),
        )
        .await?;

    result.map(|_| ())
}

/// 紐付けを開始するためのパネルを送信します。
#[poise::command(slash_command, ephemeral, guild_only)]
pub async fn create_panel(ctx: AppApplicationContext<'_>) -> Result<(), AppError> {
    let config = ctx.app_config().await;
    let config = &config.mcguildlink;
    if ctx.guild_id() != Some(config.guild_id)
        || !ctx
            .interaction
            .member
            .as_ref()
            .is_some_and(|member| member.roles.contains(&config.moderator_role_id))
    {
        return Err(BotError::HasNoRole.into());
    }

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
