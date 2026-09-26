use serde::Deserialize;
use serenity::all::{GuildId, RoleId};

#[derive(Debug, Deserialize)]
pub struct McGuildLinkConfig {
    pub guild_id: GuildId,
    pub moderator_role_id: RoleId,
    pub display_server_address: String,
}
