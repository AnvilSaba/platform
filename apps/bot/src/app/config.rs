use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use anyhow::Context as _;
use duration_str::deserialize_duration;
use serde::{Deserialize, Deserializer};
use serenity::{
    all::{ChannelId, ForumTagId, GuildId, RoleId, UserId},
    model::id::GenericChannelId,
};
use tokio::fs::read_to_string;

use crate::app::AppError;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub mcguildlink: McGuildLinkConfig,
    pub bot: BotConfig,
    pub auth: AuthConfig,
    pub auto_kick: AutoKickConfig,
    pub honeypot: HoneypotConfig,
    pub message_logging: MessageLoggingConfig,
    pub message_cache: MessageCacheConfig,
    pub pin: PinConfig,
    pub thread_auto_invite: ThreadAutoInviteConfig,
    pub question: QuestionConfig,
}

impl AppConfig {
    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref();
        let text = read_to_string(path)
            .await
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, AppError> {
        toml::from_str(text)
            .map_err(|mut error: toml::de::Error| {
                error.set_input(None);
                error
            })
            .context("Failed to parse bot configuration")
    }
}

#[derive(Debug, Deserialize)]
pub struct McGuildLinkConfig {
    pub guild_id: GuildId,
    pub moderator_role_id: RoleId,
    pub display_server_address: String,
    pub audit_channel_id: ChannelId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotConfig {
    pub owners: HashSet<UserId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub log_channel_id: ChannelId,
    pub role_id: RoleId,
    pub dummy_keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AutoKickConfig {
    pub guild_id: GuildId,
    #[serde(deserialize_with = "deserialize_duration")]
    pub grace_period: Duration,
    pub kick_message: String,
}

#[derive(Debug, Deserialize)]
pub struct HoneypotConfig {
    pub channel_id: ChannelId,
    #[serde(deserialize_with = "deserialize_duration")]
    pub message_lookback: Duration,
    pub kick_message: String,
    pub log_channel_id: ChannelId,
}

#[derive(Debug, Deserialize)]
pub struct MessageLoggingConfig {
    pub channel_id: ChannelId,
    pub snapshot_channel_id: GenericChannelId,
}

#[derive(Debug, Deserialize)]
pub struct MessageCacheConfig {
    pub disabled: bool,
    pub target_guild_ids: Vec<GuildId>,
    #[serde(deserialize_with = "deserialize_duration")]
    pub request_window: Duration,
    pub requests_per_window: u8,
    pub concurrent_channels: u8,
}

#[derive(Debug, Deserialize)]
pub struct PinConfig {
    #[serde(deserialize_with = "to_pin_channels")]
    pub channels: HashMap<ChannelId, UserId>,
}

fn to_pin_channels<'de, D>(deserializer: D) -> Result<HashMap<ChannelId, UserId>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, Deserialize)]
    struct Temp {
        id: ChannelId,
        owner: UserId,
    }

    Ok(Vec::deserialize(deserializer)?
        .into_iter()
        .map(|i: Temp| (i.id, i.owner))
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct ThreadAutoInviteConfig {
    pub display_role_id: RoleId,
    pub role_ids: Vec<RoleId>,
    pub min_member_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct QuestionConfig {
    pub forum_id: ChannelId,
    pub solved_tag: ForumTagId,
    pub solved_name_prefix: String,
}
