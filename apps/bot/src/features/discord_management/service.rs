use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use thiserror::Error;

use super::ids::{
    ChannelId, ChannelLogicalId, GuildId, MemberId, MemberLogicalId, RoleId, RoleLogicalId,
};

const SCHEMA_VERSION: u32 = 1;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceLookup {
    pub resource_type: ResourceType,
    pub guild_id: GuildId,
}

pub trait ResourceSource {
    async fn lookup_resource(
        &self,
        guild_id: &GuildId,
        discord_id: u64,
    ) -> Result<Option<ResourceLookup>, ManagementError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleSnapshot {
    pub id: RoleId,
    pub manageable: bool,
    pub name: String,
    pub color: u32,
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: BTreeMap<String, bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleCatalog {
    pub roles: Vec<RoleSnapshot>,
    pub permission_names: BTreeSet<String>,
    pub grantable_permissions: BTreeSet<String>,
    pub default_permissions: BTreeMap<String, bool>,
}

pub trait RoleSource {
    async fn role_catalog(&self, guild_id: &GuildId) -> Result<RoleCatalog, ManagementError>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoleUpdate {
    pub name: Option<String>,
    pub color: Option<u32>,
    pub hoist: Option<bool>,
    pub mentionable: Option<bool>,
    pub permissions: Option<BTreeMap<String, bool>>,
}

impl RoleUpdate {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.color.is_none()
            && self.hoist.is_none()
            && self.mentionable.is_none()
            && self.permissions.is_none()
    }

    #[cfg(test)]
    fn apply_to(&self, role: &mut RoleSnapshot) {
        if let Some(name) = &self.name {
            role.name.clone_from(name);
        }
        if let Some(color) = self.color {
            role.color = color;
        }
        if let Some(hoist) = self.hoist {
            role.hoist = hoist;
        }
        if let Some(mentionable) = self.mentionable {
            role.mentionable = mentionable;
        }
        if let Some(permissions) = &self.permissions {
            role.permissions.clone_from(permissions);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleUpdateOutcome {
    Applied,
    ResponseUnknown,
}

pub trait RoleTarget: RoleSource {
    async fn update_role(
        &self,
        guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManagementError {
    #[error("Discord から Role を取得できません: {0}")]
    RoleSource(String),
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
    #[error(
        "Discord ID {discord_id} は {expected} ではなく {actual} です"
    )]
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

#[derive(Debug, PartialEq, Eq)]
pub struct ExportFiles {
    pub definition_toml: String,
    pub state_json: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BindResult {
    pub state_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeChange {
    pub logical_id: RoleLogicalId,
    pub discord_id: RoleId,
    pub attribute: String,
    pub current: String,
    pub desired: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RolePlan {
    pub changes: Vec<AttributeChange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoleApplyStatus {
    Complete,
    GuildBusy,
    ReplanRequired,
    DeadlineExceeded,
    Failed(String),
    ResponseUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleApplyResult {
    pub status: RoleApplyStatus,
    pub applied: Vec<AttributeChange>,
    pub pending: Vec<AttributeChange>,
    pub state_json: String,
}

impl RolePlan {
    pub fn render(&self) -> String {
        if self.changes.is_empty() {
            return "変更はありません。\n".to_owned();
        }

        let mut output = String::from("Role の変更計画\n\n");
        for change in &self.changes {
            output.push_str(&format!(
                "- {} ({}) {}: {} -> {}\n",
                change.logical_id, change.discord_id, change.attribute, change.current, change.desired
            ));
        }
        output
    }
}

pub struct RoleManagementService<S> {
    source: S,
}

impl<S> RoleManagementService<S>
{
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> RoleManagementService<S>
where
    S: RoleSource,
{

    pub async fn export_roles(
        &self,
        guild_id: GuildId,
        previous_state_json: Option<&str>,
    ) -> Result<ExportFiles, ManagementError> {
        let previous_state = previous_state_json
            .map(|contents| deserialize_state_for_guild(contents, guild_id))
            .transpose()?;

        let previous_mappings = previous_state.map(|state| state.roles).unwrap_or_default();
        let previous_logical_ids = previous_mappings
            .iter()
            .map(|(logical_id, discord_id)| (*discord_id, logical_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let roles = self
            .source
            .role_catalog(&guild_id)
            .await?
            .roles
            .into_iter()
            .filter(|role| role.manageable);
        let mut definitions = BTreeMap::new();
        let mut mappings = BTreeMap::new();

        for role in roles {
            let is_everyone = role.id.get() == guild_id.get();
            let logical_id = if is_everyone {
                everyone_logical_id()
            } else if let Some(logical_id) = previous_logical_ids.get(&role.id) {
                logical_id.clone()
            } else {
                let generated = RoleLogicalId::parse(format!("role_{}", role.id))
                    .expect("Role Snowflake から生成した論理 ID は常に有効です");
                if let Some(reserved_for) = previous_mappings.get(&generated) {
                    return Err(ManagementError::InvalidState(format!(
                        "生成する論理 ID {generated} は state で Snowflake {reserved_for} に使用されています"
                    )));
                }
                generated
            };
            if !is_everyone
                && let Some(existing_id) = mappings.insert(logical_id.clone(), role.id) {
                    return Err(ManagementError::InvalidState(format!(
                        "論理 ID {logical_id} が Role {existing_id} と {} で衝突しています",
                        role.id
                    )));
                }
            definitions.insert(
                logical_id.clone(),
                RoleDefinition {
                    mode: RoleMode::Managed,
                    settings_sets: Vec::new(),
                    attributes: if is_everyone {
                        RoleAttributes {
                            permissions: role
                                .permissions
                                .into_iter()
                                .map(|(name, value)| (name, ManagedValue::Value(value)))
                                .collect(),
                            ..RoleAttributes::default()
                        }
                    } else {
                        RoleAttributes {
                            name: Some(ManagedValue::Value(role.name)),
                            color: Some(ManagedValue::Value(role.color)),
                            hoist: Some(ManagedValue::Value(role.hoist)),
                            mentionable: Some(ManagedValue::Value(role.mentionable)),
                            permissions: role
                                .permissions
                                .into_iter()
                                .filter(|(_, value)| *value)
                                .map(|(name, value)| (name, ManagedValue::Value(value)))
                                .collect(),
                        }
                    },
                },
            );
        }

        let definition_toml = toml::to_string_pretty(&DefinitionFile {
            schema_version: SCHEMA_VERSION,
            settings_sets: RoleSettingsSets::default(),
            roles: definitions,
            channels: BTreeMap::new(),
            members: BTreeMap::new(),
            message_sets: BTreeMap::new(),
            threads: BTreeMap::new(),
            order: None,
        })
        .map_err(|error| ManagementError::SerializeDefinition(error.to_string()))?;
        let state_json = serde_json::to_string_pretty(&StateFile {
            schema_version: SCHEMA_VERSION,
            guild_id,
            roles: mappings,
            channels: BTreeMap::new(),
            members: BTreeMap::new(),
        })
        .map_err(|error| ManagementError::SerializeState(error.to_string()))?;

        Ok(ExportFiles {
            definition_toml,
            state_json: format!("{state_json}\n"),
        })
    }

    pub async fn plan_roles(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
    ) -> Result<RolePlan, ManagementError> {
        let definition: DefinitionFile =
            toml::from_str(definition_toml).map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        let state = deserialize_state_for_guild(state_json, guild_id)?;
        validate_references(&definition, &state)?;
        validate_role_plan_scope(&definition)?;

        let catalog = self.source.role_catalog(&guild_id).await?;
        build_plan(&definition, &state, &catalog)
    }
}

impl<S> RoleManagementService<S>
where
    S: ResourceSource,
{
    pub async fn bind_resource(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        resource_type: ResourceType,
        logical_id: &str,
        discord_id: &str,
    ) -> Result<BindResult, ManagementError> {
        let definition: DefinitionFile = toml::from_str(definition_toml)
            .map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        let mut state = deserialize_state_for_guild(state_json, guild_id)?;
        let discord_id = discord_id.parse::<u64>().map_err(|error| {
            ManagementError::InvalidInputFile(format!(
                "{resource_type} の Discord ID {discord_id} が不正です: {error}"
            ))
        })?;
        if discord_id == u64::MAX {
            return Err(ManagementError::InvalidInputFile(format!(
                "{resource_type} の Discord ID {discord_id} は使用できません"
            )));
        }
        if resource_type == ResourceType::Role && discord_id == guild_id.get() {
            return Err(ManagementError::InvalidDefinition(
                "予約参照 everyone の Role ID は別の論理 ID へ bind できません".to_owned(),
            ));
        }

        let logical_id = match resource_type {
            ResourceType::Role => {
                let logical_id = RoleLogicalId::parse(logical_id).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Role の論理 ID が不正です: {error}"))
                })?;
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
                BoundLogicalId::Role(logical_id)
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
                BoundLogicalId::Channel(logical_id)
            }
            ResourceType::Member => {
                let logical_id = MemberLogicalId::parse(logical_id).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Member の論理 ID が不正です: {error}"))
                })?;
                if !definition.members.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidDefinition(format!(
                        "Member {logical_id} の宣言が definition にありません"
                    )));
                }
                BoundLogicalId::Member(logical_id)
            }
        };

        validate_binding_conflicts(&state, &logical_id, discord_id)?;
        let lookup = self
            .source
            .lookup_resource(&guild_id, discord_id)
            .await?
            .ok_or(ManagementError::ResourceNotFound {
                resource_type,
                discord_id,
            })?;
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

        match logical_id {
            BoundLogicalId::Role(logical_id) => {
                state.roles.insert(logical_id, RoleId::new(discord_id));
            }
            BoundLogicalId::Channel(logical_id) => {
                state.channels.insert(logical_id, ChannelId::new(discord_id));
            }
            BoundLogicalId::Member(logical_id) => {
                state.members.insert(logical_id, MemberId::new(discord_id));
            }
        }

        Ok(BindResult {
            state_json: serialize_state(&state)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BoundLogicalId {
    Role(RoleLogicalId),
    Channel(ChannelLogicalId),
    Member(MemberLogicalId),
}

fn validate_binding_conflicts(
    state: &StateFile,
    logical_id: &BoundLogicalId,
    discord_id: u64,
) -> Result<(), ManagementError> {
    match logical_id {
        BoundLogicalId::Role(logical_id) => {
            if let Some(existing) = state.roles.get(logical_id)
                && existing.get() != discord_id
            {
                return Err(ManagementError::InvalidState(format!(
                    "Role {logical_id} はすでに Snowflake {existing} に対応しており、{discord_id} へ暗黙に付け替えられません"
                )));
            }
            if let Some((existing, _)) = state.roles.iter().find(|(_, value)| value.get() == discord_id)
                && existing != logical_id
            {
                return Err(ManagementError::InvalidState(format!(
                    "Role {existing} がすでに Snowflake {discord_id} を採用しています"
                )));
            }
        }
        BoundLogicalId::Channel(logical_id) => {
            if let Some(existing) = state.channels.get(logical_id)
                && existing.get() != discord_id
            {
                return Err(ManagementError::InvalidState(format!(
                    "Channel {logical_id} はすでに Snowflake {existing} に対応しており、{discord_id} へ暗黙に付け替えられません"
                )));
            }
            if let Some((existing, _)) = state.channels.iter().find(|(_, value)| value.get() == discord_id)
                && existing != logical_id
            {
                return Err(ManagementError::InvalidState(format!(
                    "Channel {existing} がすでに Snowflake {discord_id} を採用しています"
                )));
            }
        }
        BoundLogicalId::Member(logical_id) => {
            if let Some(existing) = state.members.get(logical_id)
                && existing.get() != discord_id
            {
                return Err(ManagementError::InvalidState(format!(
                    "Member {logical_id} はすでに Snowflake {existing} に対応しており、{discord_id} へ暗黙に付け替えられません"
                )));
            }
            if let Some((existing, _)) = state.members.iter().find(|(_, value)| value.get() == discord_id)
                && existing != logical_id
            {
                return Err(ManagementError::InvalidState(format!(
                    "Member {existing} がすでに Snowflake {discord_id} を採用しています"
                )));
            }
        }
    }
    Ok(())
}


fn build_plan(
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &RoleCatalog,
) -> Result<RolePlan, ManagementError> {
    validate_permission_names(definition, &catalog.permission_names)?;
    let actual_roles = catalog
        .roles
        .iter()
        .map(|role| (role.id, role))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();

    for (logical_id, desired) in &definition.roles {
        let discord_id = resolve_role_id(logical_id, state)?;
        let actual = actual_roles.get(&discord_id).ok_or_else(|| {
            ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} が Guild に存在しません"
            ))
        })?;
        let desired_attributes = compose_attributes(desired, &definition.settings_sets.role);
        if discord_id.get() == state.guild_id.get() && desired_attributes.has_non_permission_attributes() {
            return Err(ManagementError::InvalidDefinition(
                "@everyone Role では権限だけを管理できます".to_owned(),
            ));
        }
        if matches!(desired.mode, RoleMode::Managed) && !actual.manageable {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} は Bot が管理できません"
            )));
        }
        compare_attributes(
            logical_id,
            &discord_id,
            actual,
            &desired_attributes,
            &catalog.default_permissions,
            &catalog.grantable_permissions,
            &mut changes,
        )?;
    }

    Ok(RolePlan { changes })
}

mod apply;


mod model;

use model::*;


#[cfg(test)]
#[path = "service/tests.rs"]
mod tests;
