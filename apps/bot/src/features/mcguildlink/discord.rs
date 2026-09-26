use super::{ports::LinkReply, postgres::PostgresLinkCodes, start_link::start_link};
use crate::app::{AppApplicationContext, AppError, BotDataExt};
use bot_macros::event_handler;
use poise::CreateReply;
use serenity::{
    all::{
        ButtonStyle, ComponentInteraction, ComponentInteractionDataKind, Context, CreateActionRow,
        CreateAllowedMentions, CreateButton, CreateMessage, EditInteractionResponse, FullEvent, Interaction,
    },
    async_trait,
    builder::CreateComponent,
};

const START_LINK_BUTTON_ID: &str = "start_link_button";

struct DiscordReply<'a> {
    ctx: &'a Context,
    interaction: &'a ComponentInteraction,
}

#[async_trait]
impl LinkReply for DiscordReply<'_> {
    async fn defer_ephemeral(&self) -> Result<(), AppError> {
        self.interaction.defer_ephemeral(&self.ctx.http).await?;
        Ok(())
    }

    async fn complete(&self, content: String) -> Result<(), AppError> {
        self.interaction
            .edit_response(
                &self.ctx.http,
                EditInteractionResponse::new()
                    .content(content)
                    .allowed_mentions(CreateAllowedMentions::new()),
            )
            .await?;
        Ok(())
    }
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
    start_link(
        &PostgresLinkCodes::new(data.database.clone()),
        &DiscordReply { ctx, interaction },
        interaction.user.id.get(),
        &interaction.user.name,
        &config.display_server_address,
    )
    .await
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
        ctx.send(CreateReply::default().content("この操作を実行する権限がありません。"))
            .await?;
        return Ok(());
    }
    ctx.defer_ephemeral().await?;
    ctx.channel_id().send_message(ctx.http(), CreateMessage::new()
        .content("Minecraftアカウントと Discordアカウントを紐付けます。\n「MCアカウントと紐付ける」ボタンを押して、指示に従ってください。")
        .components(&[CreateComponent::ActionRow(CreateActionRow::buttons(&[
            CreateButton::new(START_LINK_BUTTON_ID).label("MCアカウントと紐付ける").style(ButtonStyle::Primary)
        ]))])).await?;
    ctx.send(CreateReply::default().content("パネルを作成しました！"))
        .await?;
    Ok(())
}
