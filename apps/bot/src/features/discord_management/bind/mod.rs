use std::{collections::BTreeMap, fmt};

use super::{
    configuration::{DefinitionFile, StateFile, everyone_logical_id, serialize_state},
    domain::{ManagementError, ResourceType},
    ids::{ChannelId, ChannelLogicalId, GuildId, MemberId, MemberLogicalId, RoleId, RoleLogicalId},
    port::{ResourceLookup, ResourceSource},
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct BindResult {
    pub state_json: String,
}

pub(super) async fn bind_resource<S: ResourceSource>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    resource_type: ResourceType,
    logical_id: &str,
    discord_id: &str,
) -> Result<BindResult, ManagementError> {
    let definition = DefinitionFile::parse(definition_toml)?;
    let mut state = StateFile::parse_for_guild(state_json, guild_id)?;
    let discord_id = parse_discord_id(resource_type, discord_id)?;
    if resource_type == ResourceType::Role && discord_id == guild_id.get() {
        return Err(ManagementError::InvalidDefinition(
            "予約参照 everyone の Role ID は別の論理 ID へ bind できません".to_owned(),
        ));
    }

    let logical_id = declared_logical_id(&definition, resource_type, logical_id)?;
    validate_binding_conflicts(&state, &logical_id, discord_id)?;
    let lookup = source
        .lookup_resource(&guild_id, discord_id)
        .await?
        .ok_or(ManagementError::ResourceNotFound {
            resource_type,
            discord_id,
        })?;
    validate_lookup(resource_type, discord_id, guild_id, lookup)?;
    logical_id.insert_into(&mut state, discord_id);

    Ok(BindResult {
        state_json: serialize_state(&state)?,
    })
}

fn parse_discord_id(resource_type: ResourceType, discord_id: &str) -> Result<u64, ManagementError> {
    let parsed = discord_id.parse::<u64>().map_err(|error| {
        ManagementError::InvalidInputFile(format!(
            "{resource_type} の Discord ID {discord_id} が不正です: {error}"
        ))
    })?;
    if parsed == u64::MAX {
        return Err(ManagementError::InvalidInputFile(format!(
            "{resource_type} の Discord ID {discord_id} は使用できません"
        )));
    }
    Ok(parsed)
}

fn declared_logical_id(
    definition: &DefinitionFile,
    resource_type: ResourceType,
    logical_id: &str,
) -> Result<BoundLogicalId, ManagementError> {
    match resource_type {
        ResourceType::Role => {
            let logical_id = RoleLogicalId::parse(logical_id)
                .map_err(|error| ManagementError::InvalidDefinition(format!("Role の論理 ID が不正です: {error}")))?;
            if logical_id == everyone_logical_id() {
                return Err(ManagementError::InvalidDefinition(
                    "予約参照 everyone は bind せず、Guild ID へ自動解決します".to_owned(),
                ));
            }
            if !definition.roles.contains_key(&logical_id) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Role {logical_id} の宣言が definition にありません"
                )));
            }
            Ok(BoundLogicalId::Role(logical_id))
        }
        ResourceType::Channel => {
            let logical_id = ChannelLogicalId::parse(logical_id).map_err(|error| {
                ManagementError::InvalidDefinition(format!("Channel の論理 ID が不正です: {error}"))
            })?;
            let Some(declaration) = definition.channels.get(&logical_id) else {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} の宣言が definition にありません"
                )));
            };
            if declaration.is_absent() {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} は削除宣言のため bind できません"
                )));
            }
            Ok(BoundLogicalId::Channel(logical_id))
        }
        ResourceType::Member => {
            let logical_id = MemberLogicalId::parse(logical_id)
                .map_err(|error| ManagementError::InvalidDefinition(format!("Member の論理 ID が不正です: {error}")))?;
            if !definition.members.contains_key(&logical_id) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Member {logical_id} の宣言が definition にありません"
                )));
            }
            Ok(BoundLogicalId::Member(logical_id))
        }
    }
}

fn validate_lookup(
    resource_type: ResourceType,
    discord_id: u64,
    guild_id: GuildId,
    lookup: ResourceLookup,
) -> Result<(), ManagementError> {
    if lookup.resource_type != resource_type {
        return Err(ManagementError::ResourceTypeMismatch {
            expected: resource_type,
            actual: lookup.resource_type,
            discord_id,
        });
    }
    if lookup.guild_id != guild_id {
        return Err(ManagementError::ResourceGuildMismatch {
            resource_type,
            discord_id,
            resource_guild_id: lookup.guild_id,
            actual_guild_id: guild_id,
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BoundLogicalId {
    Role(RoleLogicalId),
    Channel(ChannelLogicalId),
    Member(MemberLogicalId),
}

impl BoundLogicalId {
    fn insert_into(self, state: &mut StateFile, discord_id: u64) {
        match self {
            Self::Role(logical_id) => {
                state.roles.insert(logical_id, RoleId::new(discord_id));
            }
            Self::Channel(logical_id) => {
                state.channels.insert(logical_id, ChannelId::new(discord_id));
            }
            Self::Member(logical_id) => {
                state.members.insert(logical_id, MemberId::new(discord_id));
            }
        }
    }
}

fn validate_binding_conflicts(
    state: &StateFile,
    logical_id: &BoundLogicalId,
    discord_id: u64,
) -> Result<(), ManagementError> {
    match logical_id {
        BoundLogicalId::Role(logical_id) => {
            validate_mapping_conflict(&state.roles, logical_id, discord_id, "Role", RoleId::get)
        }
        BoundLogicalId::Channel(logical_id) => {
            validate_mapping_conflict(&state.channels, logical_id, discord_id, "Channel", ChannelId::get)
        }
        BoundLogicalId::Member(logical_id) => {
            validate_mapping_conflict(&state.members, logical_id, discord_id, "Member", MemberId::get)
        }
    }
}

fn validate_mapping_conflict<LogicalId, DiscordId, GetId>(
    mappings: &BTreeMap<LogicalId, DiscordId>,
    logical_id: &LogicalId,
    discord_id: u64,
    resource_type: &str,
    get_id: GetId,
) -> Result<(), ManagementError>
where
    LogicalId: Eq + Ord + fmt::Display,
    DiscordId: Copy + fmt::Display,
    GetId: Fn(DiscordId) -> u64,
{
    if let Some(existing) = mappings.get(logical_id)
        && get_id(*existing) != discord_id
    {
        return Err(ManagementError::InvalidState(format!(
            "{resource_type} {logical_id} はすでに Snowflake {existing} に対応しており、{discord_id} へ暗黙に付け替えられません"
        )));
    }
    if let Some((existing, _)) = mappings.iter().find(|(_, value)| get_id(**value) == discord_id)
        && existing != logical_id
    {
        return Err(ManagementError::InvalidState(format!(
            "{resource_type} {existing} がすでに Snowflake {discord_id} を採用しています"
        )));
    }
    Ok(())
}
