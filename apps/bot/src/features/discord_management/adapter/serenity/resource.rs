use serenity::{
    Error as SerenityError,
    all::{GuildId as SerenityGuildId, Http, RoleId as SerenityRoleId, UserId},
    http::StatusCode,
};

use crate::features::discord_management::domain::{ManagementError, ResourceType};
use crate::features::discord_management::ids::GuildId;
use crate::features::discord_management::port::{ResourceLookup, ResourceSource};

impl From<SerenityGuildId> for GuildId {
    fn from(id: SerenityGuildId) -> Self {
        Self::new(id.get())
    }
}

impl From<GuildId> for SerenityGuildId {
    fn from(id: GuildId) -> Self {
        Self::new(id.get())
    }
}

pub(crate) struct SerenityRoleSource<'a> {
    pub(super) http: &'a Http,
    pub(super) bot_user_id: UserId,
}

impl<'a> SerenityRoleSource<'a> {
    pub(crate) fn new(http: &'a Http, bot_user_id: UserId) -> Self {
        Self { http, bot_user_id }
    }
}

impl ResourceSource for SerenityRoleSource<'_> {
    async fn lookup_resource(
        &self,
        guild_id: &GuildId,
        discord_id: u64,
    ) -> Result<Option<ResourceLookup>, ManagementError> {
        let guild_id = SerenityGuildId::from(*guild_id);
        let channels = guild_id
            .channels(self.http)
            .await
            .map_err(|error| ManagementError::ResourceSource(error.to_string()))?;
        if channels.into_iter().any(|channel| channel.id.get() == discord_id) {
            return Ok(Some(ResourceLookup {
                resource_type: ResourceType::Channel,
                guild_id: GuildId::from(guild_id),
            }));
        }

        let roles = guild_id
            .roles(self.http)
            .await
            .map_err(|error| ManagementError::ResourceSource(error.to_string()))?;
        if roles.contains_key(&SerenityRoleId::new(discord_id)) {
            return Ok(Some(ResourceLookup {
                resource_type: ResourceType::Role,
                guild_id: GuildId::from(guild_id),
            }));
        }

        match guild_id.member(self.http, UserId::new(discord_id)).await {
            Ok(_) => {
                return Ok(Some(ResourceLookup {
                    resource_type: ResourceType::Member,
                    guild_id: GuildId::from(guild_id),
                }));
            }
            Err(SerenityError::Http(error)) if error.status_code() == Some(StatusCode::NOT_FOUND) => {}
            Err(error) => return Err(ManagementError::ResourceSource(error.to_string())),
        }

        Ok(None)
    }
}
