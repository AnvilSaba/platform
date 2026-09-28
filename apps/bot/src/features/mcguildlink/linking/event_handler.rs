use super::{ports::LinkCodeResult, service::LinkCodes, types::DiscordUserId};
use crate::{
    app::{AppError, BotDataExt},
    core::BotEventHandler,
    features::mcguildlink::repository::DatabaseMcGuildLinkRepository,
    utils::create_safe_allowed_mentions,
};
use serenity::{
    all::{
        ComponentInteraction, ComponentInteractionDataKind, Context, EditInteractionResponse, FullEvent, Interaction,
    },
    async_trait,
};
use sqlx::PgPool;

pub(super) const START_LINK_BUTTON_ID: &str = "start_link_button";

pub struct LinkCodeEventHandler {
    codes: LinkCodes<DatabaseMcGuildLinkRepository>,
}

impl LinkCodeEventHandler {
    pub fn new(database: &PgPool) -> Self {
        Self {
            codes: LinkCodes::new(DatabaseMcGuildLinkRepository::new(database.clone())),
        }
    }

    async fn handle_component(&self, ctx: &Context, interaction: &ComponentInteraction) -> Result<(), AppError> {
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

        interaction.defer_ephemeral(&ctx.http).await?;
        let result = self
            .codes
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
}

#[async_trait]
impl BotEventHandler for LinkCodeEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        if let FullEvent::InteractionCreate {
            interaction: Interaction::Component(interaction),
            ..
        } = event
        {
            self.handle_component(ctx, interaction).await?;
        }
        Ok(())
    }
}
