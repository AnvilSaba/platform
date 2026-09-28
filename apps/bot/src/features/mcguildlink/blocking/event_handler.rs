use super::{
    presentation,
    repository::{BlockCause, DatabaseBlockRepository},
};
use crate::{
    app::{AppError, BotDataExt},
    core::BotEventHandler,
    utils::{create_ephemeral_message, create_safe_allowed_mentions},
};
use serenity::{
    all::{ComponentInteractionDataKind, Context, FullEvent, Interaction, MessageFlags},
    async_trait,
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
};
use sqlx::PgPool;

pub struct BlockingEventHandler {
    store: DatabaseBlockRepository,
}

impl BlockingEventHandler {
    pub fn new(database: &PgPool) -> Self {
        Self {
            store: DatabaseBlockRepository::new(database.clone()),
        }
    }

    pub async fn on_ban(
        &self,
        configured_guild: u64,
        event_guild: u64,
        user_id: u64,
        username: &str,
    ) -> Result<(), AppError> {
        if configured_guild == event_guild {
            self.store.block(user_id, username, BlockCause::GuildBan).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl BotEventHandler for BlockingEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        let config = ctx.app_config().await;
        match event {
            FullEvent::GuildBanAddition {
                guild_id, banned_user, ..
            } => {
                self.on_ban(
                    config.mcguildlink.guild_id.get(),
                    guild_id.get(),
                    banned_user.id.get(),
                    &banned_user.name,
                )
                .await?;
            }
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(component),
                ..
            } if component.guild_id == Some(config.mcguildlink.guild_id)
                && matches!(component.data.kind, ComponentInteractionDataKind::Button) =>
            {
                let Some((id, page)) = presentation::parse(&component.data.custom_id) else {
                    return Ok(());
                };
                let Some(snapshot) = presentation::get(id) else {
                    component
                        .create_response(
                            &ctx.http,
                            create_ephemeral_message("一覧の有効期限が切れました。もう一度開き直してください。", None),
                        )
                        .await?;
                    return Ok(());
                };
                if snapshot.owner != component.user.id.get()
                    || !component
                        .member
                        .as_ref()
                        .is_some_and(|member| member.roles.contains(&config.mcguildlink.moderator_role_id))
                {
                    component
                        .create_response(&ctx.http, create_ephemeral_message("管理者権限が必要です。", None))
                        .await?;
                    return Ok(());
                }
                component
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new()
                                .flags(MessageFlags::IS_COMPONENTS_V2)
                                .components(presentation::render(id, &snapshot, page))
                                .allowed_mentions(create_safe_allowed_mentions()),
                        ),
                    )
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }
}
