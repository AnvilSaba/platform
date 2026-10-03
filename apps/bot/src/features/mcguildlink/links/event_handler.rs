use serenity::{
    all::{ComponentInteraction, ComponentInteractionDataKind, Context, FullEvent, Interaction, ModalInteraction},
    async_trait,
};
use sqlx::PgPool;

use super::LIST_LINK_BUTTON_ID;
use super::presentation::parse_page;
use super::{
    blocking::{interactions as block_interactions, presentation as block_presentation},
    interactions::{self, UNLINK_BUTTON_PREFIX, UNLINK_MODAL_PREFIX},
    repository::DatabaseAccountLinksRepository,
};
use crate::{
    app::{AppError, BotDataExt},
    core::BotEventHandler,
};

pub struct AccountLinksEventHandler {
    repository: DatabaseAccountLinksRepository,
}

impl AccountLinksEventHandler {
    pub fn new(database: &PgPool) -> Self {
        Self {
            repository: DatabaseAccountLinksRepository::new(database.clone()),
        }
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
            interactions::show_link_list(&self.repository, ctx, component).await?;
        } else if let Some((snapshot_id, page_index)) = parse_page(id) {
            interactions::show_page(ctx, component, snapshot_id, page_index).await?;
        } else if let Some((snapshot_id, page_index)) = block_presentation::parse(id) {
            block_interactions::show_page(ctx, component, snapshot_id, page_index).await?;
        } else if let Some((owner, uuid)) = interactions::parse_unlink(id, UNLINK_BUTTON_PREFIX) {
            interactions::show_unlink_confirmation(&self.repository, ctx, component, owner, uuid).await?;
        }
        Ok(())
    }

    async fn handle_modal(&self, ctx: &Context, modal: &ModalInteraction) -> Result<(), AppError> {
        let configured_guild = ctx.app_config().await.mcguildlink.guild_id;
        if modal.guild_id != Some(configured_guild) {
            return Ok(());
        }
        if let Some((owner, uuid)) = interactions::parse_unlink(&modal.data.custom_id, UNLINK_MODAL_PREFIX) {
            interactions::complete_unlink(&self.repository, ctx, modal, owner, uuid).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl BotEventHandler for AccountLinksEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        match event {
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
