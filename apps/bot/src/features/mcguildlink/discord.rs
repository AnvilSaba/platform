use super::{
    ports::{LinkCodeResult, LinkCodes},
    adapter::DatabaseLinkCodes,
    types::DiscordUserId,
};
use crate::{
    app::{AppApplicationContext, AppError, BotDataExt},
    utils::{create_safe_allowed_mentions, create_safe_message},
};
use bot_macros::event_handler;
use poise::CreateReply;
use serenity::{
    all::{
        ButtonStyle, ComponentInteractionDataKind, Context, CreateActionRow, CreateButton, EditInteractionResponse,
        FullEvent, Interaction,
    },
    builder::CreateComponent,
};

const START_LINK_BUTTON_ID: &str = "start_link_button";

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
    let result = DatabaseLinkCodes::new(data.database.clone())
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
        ctx.send(
            CreateReply::default()
                .allowed_mentions(create_safe_allowed_mentions())
                .content("この操作を実行する権限がありません。"),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;

    ctx.channel_id().send_message(ctx.http(), create_safe_message()
        .content("Minecraftアカウントと Discordアカウントを紐付けます。\n「MCアカウントと紐付ける」ボタンを押して、指示に従ってください。")
        .components(&[CreateComponent::ActionRow(CreateActionRow::buttons(&[
            CreateButton::new(START_LINK_BUTTON_ID).label("MCアカウントと紐付ける").style(ButtonStyle::Primary)
        ]))])).await?;

    ctx.send(
        CreateReply::default()
            .allowed_mentions(create_safe_allowed_mentions())
            .content("パネルを作成しました！"),
    )
    .await?;

    Ok(())
}
