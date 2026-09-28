use super::{
    interactions::{self, UNLINK_BUTTON_PREFIX, UNLINK_MODAL_PREFIX},
    store::LinkManagement,
};
use crate::{
    app::{AppError, BotDataExt},
    core::BotEventHandler,
};
use serenity::{
    all::{
        ComponentInteraction, ComponentInteractionDataKind, Context, FullEvent, GuildId, Interaction, ModalInteraction,
    },
    async_trait,
};
use sqlx::PgPool;

use super::super::linking::LIST_LINK_BUTTON_ID;
use super::presentation::parse_page;

pub struct LinkManagementEventHandler {
    store: LinkManagement,
}

impl LinkManagementEventHandler {
    pub fn new(database: &PgPool) -> Self {
        Self {
            store: LinkManagement::new(database.clone()),
        }
    }

    async fn handle_member_leave(
        &self,
        ctx: &Context,
        event_guild: GuildId,
        user_id: u64,
        username: &str,
    ) -> Result<(), AppError> {
        let configured_guild = ctx.app_config().await.mcguildlink.guild_id;
        self.handle_member_leave_for_guild(configured_guild, event_guild, user_id, username)
            .await
    }

    pub(super) async fn handle_member_leave_for_guild(
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

    async fn handle_component(&self, ctx: &Context, component: &ComponentInteraction) -> Result<(), AppError> {
        let config = ctx.app_config().await;
        if component.guild_id != Some(config.mcguildlink.guild_id)
            || !matches!(component.data.kind, ComponentInteractionDataKind::Button)
        {
            return Ok(());
        }

        let id = component.data.custom_id.as_str();
        if id == LIST_LINK_BUTTON_ID {
            interactions::show_link_list(&self.store, ctx, component).await?;
        } else if let Some((snapshot_id, page_index)) = parse_page(id) {
            interactions::show_page(
                ctx,
                component,
                config.mcguildlink.moderator_role_id,
                snapshot_id,
                page_index,
            )
            .await?;
        } else if let Some((owner, uuid)) = interactions::parse_unlink(id, UNLINK_BUTTON_PREFIX) {
            interactions::show_unlink_confirmation(&self.store, ctx, component, owner, uuid).await?;
        }
        Ok(())
    }

    async fn handle_modal(&self, ctx: &Context, modal: &ModalInteraction) -> Result<(), AppError> {
        let configured_guild = ctx.app_config().await.mcguildlink.guild_id;
        if modal.guild_id != Some(configured_guild) {
            return Ok(());
        }
        if let Some((owner, uuid)) = interactions::parse_unlink(&modal.data.custom_id, UNLINK_MODAL_PREFIX) {
            interactions::complete_unlink(&self.store, ctx, modal, owner, uuid).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl BotEventHandler for LinkManagementEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        match event {
            FullEvent::GuildMemberRemoval { guild_id, user, .. } => {
                self.handle_member_leave(ctx, *guild_id, user.id.get(), &user.name)
                    .await?;
            }
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(component),
                ..
            } => {
                self.handle_component(ctx, component).await?;
            }
            FullEvent::InteractionCreate {
                interaction: Interaction::Modal(modal),
                ..
            } => {
                self.handle_modal(ctx, modal).await?;
            }
            _ => {}
        }
        Ok(())
    }
}
