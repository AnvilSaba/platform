use super::super::links::blocking::repository::{BlockCause, DatabaseBlockRepository};
use super::leave::{DatabaseMemberDepartureRepository, MemberDepartureRepository};
use crate::{
    app::{AppError, BotDataExt},
    core::BotEventHandler,
};
use serenity::{
    all::{Context, FullEvent},
    async_trait,
};
use sqlx::PgPool;

pub struct GuildMembershipEventHandler {
    departure: DatabaseMemberDepartureRepository,
    blocking: DatabaseBlockRepository,
}

impl GuildMembershipEventHandler {
    pub fn new(database: &PgPool) -> Self {
        Self {
            departure: DatabaseMemberDepartureRepository::new(database.clone()),
            blocking: DatabaseBlockRepository::new(database.clone()),
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
            self.blocking.block(user_id, username, BlockCause::GuildBan).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl BotEventHandler for GuildMembershipEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        let configured_guild = ctx.app_config().await.mcguildlink.guild_id;
        match event {
            FullEvent::GuildMemberRemoval { guild_id, user, .. } if *guild_id == configured_guild => {
                self.departure.member_left(user.id.get(), &user.name).await?;
            }
            FullEvent::GuildBanAddition {
                guild_id, banned_user, ..
            } => {
                self.on_ban(
                    configured_guild.get(),
                    guild_id.get(),
                    banned_user.id.get(),
                    &banned_user.name,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }
}
