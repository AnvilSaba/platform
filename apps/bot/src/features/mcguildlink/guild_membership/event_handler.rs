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
}

impl GuildMembershipEventHandler {
    pub fn new(database: &PgPool) -> Self {
        Self {
            departure: DatabaseMemberDepartureRepository::new(database.clone()),
        }
    }
}

#[async_trait]
impl BotEventHandler for GuildMembershipEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        if let FullEvent::GuildMemberRemoval { guild_id, user, .. } = event
            && *guild_id == ctx.app_config().await.mcguildlink.guild_id
        {
            self.departure.member_left(user.id.get(), &user.name).await?;
        }
        Ok(())
    }
}
