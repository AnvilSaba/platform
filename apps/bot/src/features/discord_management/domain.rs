use std::{fmt, str::FromStr};

use thiserror::Error;

use super::ids::GuildId;

pub(super) const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResourceType {
    Role,
    Channel,
    Member,
}

impl ResourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Role => "Role",
            Self::Channel => "Channel",
            Self::Member => "Member",
        }
    }
}

impl fmt::Display for ResourceType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ResourceType {
    type Err = ManagementError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "role" => Ok(Self::Role),
            "channel" => Ok(Self::Channel),
            "member" => Ok(Self::Member),
            _ => Err(ManagementError::InvalidInputFile(format!(
                "リソース種別 {value} は role、channel、member のいずれかで指定してください"
            ))),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManagementError {
    #[error("Discord から Role を取得できません: {0}")]
    RoleSource(String),
    #[error("Discord から Role を取得する権限が不足しています: {0}")]
    RoleCatalogPermissionDenied(String),
    #[error("Role の操作権限が不足しています: {0}")]
    RolePermissionDenied(String),
    #[error("Discord から Channel を取得できません: {0}")]
    ChannelSource(String),
    #[error("Discord から Channel を取得する権限が不足しています: {0}")]
    ChannelCatalogPermissionDenied(String),
    #[error("Channel の操作権限が不足しています: {0}")]
    ChannelPermissionDenied(String),
    #[error("Discord から bind 対象を取得できません: {0}")]
    ResourceSource(String),
    #[error("定義ファイルを生成できません: {0}")]
    SerializeDefinition(String),
    #[error("state ファイルを生成できません: {0}")]
    SerializeState(String),
    #[error("state ファイルが不正です: {0}")]
    InvalidState(String),
    #[error("定義ファイルが不正です: {0}")]
    InvalidDefinition(String),
    #[error("入力ファイルが不正です: {0}")]
    InvalidInputFile(String),
    #[error("state の Guild {state_guild_id} は実行 Guild {actual_guild_id} と一致しません")]
    GuildMismatch {
        state_guild_id: GuildId,
        actual_guild_id: GuildId,
    },
    #[error("{resource_type} の Discord ID {discord_id} が見つかりません")]
    ResourceNotFound {
        resource_type: ResourceType,
        discord_id: u64,
    },
    #[error("Discord ID {discord_id} は {expected} ではなく {actual} です")]
    ResourceTypeMismatch {
        expected: ResourceType,
        actual: ResourceType,
        discord_id: u64,
    },
    #[error(
        "{resource_type} の Discord ID {discord_id} は Guild {resource_guild_id} に属し、実行 Guild {actual_guild_id} と一致しません"
    )]
    ResourceGuildMismatch {
        resource_type: ResourceType,
        discord_id: u64,
        resource_guild_id: GuildId,
        actual_guild_id: GuildId,
    },
}
