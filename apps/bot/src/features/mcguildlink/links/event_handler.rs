use super::store::LinkManagement;
use crate::{
    app::{AppError, BotDataExt},
    core::BotEventHandler,
};
use serenity::{
    all::{ComponentInteractionDataKind, Context, FullEvent, GuildId, Interaction},
    async_trait,
};
use sqlx::PgPool;

impl LinkManagementEventHandler {
    pub fn new(database: &PgPool) -> Self {
        Self {
            store: LinkManagement::new(database.clone()),
        }
    }

    pub(super) async fn handle_member_leave(
        &self,
        configured_guild: GuildId,
        event_guild: GuildId,
        user_id: u64,
        username: &str,
    ) -> Result<(), AppError> {
        if event_guild == configured_guild {
            self.store.member_left(user_id, username).await?;
        }
        Ok(())
    }
}
pub struct LinkManagementEventHandler {
    pub(super) store: LinkManagement,
}

#[async_trait]
impl BotEventHandler for LinkManagementEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        let config = ctx.app_config().await;
        let guild_id = config.mcguildlink.guild_id;
        match event {
            FullEvent::GuildMemberRemoval {
                guild_id: event_guild,
                user,
                ..
            } => {
                self.handle_member_leave(guild_id, *event_guild, user.id.get(), &user.name)
                    .await?;
            }
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(component),
                ..
            } if component.guild_id == Some(guild_id)
                && matches!(component.data.kind, ComponentInteractionDataKind::Button) =>
            {
                self.handle_component(ctx, component, config.mcguildlink.moderator_role_id)
                    .await?;
            }
            FullEvent::InteractionCreate {
                interaction: Interaction::Modal(modal),
                ..
            } if modal.guild_id == Some(guild_id) => {
                self.handle_unlink_modal(ctx, modal).await?;
            }
            _ => {}
        }
        Ok(())
    }
}
