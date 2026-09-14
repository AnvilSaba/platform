use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use thiserror::Error;

use super::ids::{
    ChannelId, ChannelLogicalId, GuildId, MemberId, MemberLogicalId, RoleId, RoleLogicalId, RoleSettingsSetId,
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
    pub color: Color,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleCreate {
    pub name: String,
    pub color: Color,
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: BTreeMap<String, bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleCreateOutcome {
    Created(RoleId),
    ResponseUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleDeleteOutcome {
    Deleted,
    ResponseUnknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoleUpdate {
    pub name: Option<String>,
    pub color: Option<Color>,
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

pub trait RoleUpdater: RoleSource {
    async fn update_role(
        &self,
        guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError>;
}

pub trait RoleLifecycleTarget: RoleUpdater {
    async fn create_role(&self, guild_id: &GuildId, create: RoleCreate) -> Result<RoleCreateOutcome, ManagementError>;

    async fn delete_role(&self, guild_id: &GuildId, role_id: &RoleId) -> Result<RoleDeleteOutcome, ManagementError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManagementError {
    #[error("Discord から Role を取得できません: {0}")]
    RoleSource(String),
    #[error("Discord から Role を取得する権限が不足しています: {0}")]
    RoleCatalogPermissionDenied(String),
    #[error("Role の操作権限が不足しています: {0}")]
    RolePermissionDenied(String),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoleLifecycleChange {
    Create {
        logical_id: RoleLogicalId,
        recreated: bool,
    },
    Release {
        logical_id: RoleLogicalId,
        discord_id: RoleId,
    },
    Delete {
        logical_id: RoleLogicalId,
        discord_id: RoleId,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RolePlan {
    pub changes: Vec<AttributeChange>,
    pub lifecycle: Vec<RoleLifecycleChange>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoleApplyOptions {
    pub allow_deletions: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoleApplyStatus {
    Complete,
    GuildBusy,
    ReplanRequired,
    DeadlineExceeded,
    DeletionPermissionRequired,
    DeletionPermissionDenied(String),
    DeletionVerificationPermissionDenied(String),
    CreationResponseUnknown,
    DeletionResponseUnknown,
    DeletionVerificationIndeterminate(String),
    Failed(String),
    ResponseUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleApplyResult {
    pub status: RoleApplyStatus,
    pub applied: Vec<AttributeChange>,
    pub pending: Vec<AttributeChange>,
    pub applied_lifecycle: Vec<RoleLifecycleChange>,
    pub pending_lifecycle: Vec<RoleLifecycleChange>,
    pub state_json: String,
}

impl RolePlan {
    pub fn render(&self) -> String {
        if self.changes.is_empty() && self.lifecycle.is_empty() {
            return "変更はありません。\n".to_owned();
        }

        let mut output = String::from("Role の変更計画\n\n");
        for change in &self.lifecycle {
            match change {
                RoleLifecycleChange::Create {
                    logical_id,
                    recreated,
                } => {
                    let action = if *recreated { "再作成" } else { "新規作成" };
                    output.push_str(&format!("- {action}: {logical_id}\n"));
                }
                RoleLifecycleChange::Release {
                    logical_id,
                    discord_id,
                } => output.push_str(&format!("- 管理解除: {logical_id} ({discord_id})\n")),
                RoleLifecycleChange::Delete {
                    logical_id,
                    discord_id,
                } => output.push_str(&format!(
                    "- 削除: {logical_id} ({discord_id})\n  影響: Guild から Role が削除され、付与済みの割り当ても失われます。\n"
                )),
            }
        }
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

impl<S> RoleManagementService<S> {
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
            .map(|contents| StateFile::parse_for_guild(contents, guild_id))
            .transpose()?;

        let previous_mappings = previous_state.map(StateFile::into_role_mappings).unwrap_or_default();
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
            if !is_everyone && let Some(existing_id) = mappings.insert(logical_id.clone(), role.id) {
                return Err(ManagementError::InvalidState(format!(
                    "論理 ID {logical_id} が Role {existing_id} と {} で衝突しています",
                    role.id
                )));
            }
            definitions.insert(
                logical_id.clone(),
                RawRoleDefinition {
                    ensure: RoleEnsure::Present,
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

        let definition_toml = toml::to_string_pretty(&RawDefinitionFile {
            schema_version: SCHEMA_VERSION,
            settings_sets: SettingsSets::default(),
            roles: definitions,
            channels: BTreeMap::new(),
            members: BTreeMap::new(),
            message_sets: BTreeMap::new(),
            threads: BTreeMap::new(),
            order: None,
        })
        .map_err(|error| ManagementError::SerializeDefinition(error.to_string()))?;
        let state_json = serde_json::to_string_pretty(&RawStateFile {
            schema_version: SCHEMA_VERSION,
            guild_id,
            roles: mappings,
            channels: BTreeMap::new(),
            members: BTreeMap::new(),
            deleted_roles: BTreeSet::new(),
            pending_creations: BTreeSet::new(),
            pending_deletions: BTreeSet::new(),
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
        let input = PlanInput::parse(definition_toml, state_json, guild_id)?;

        let catalog = self.source.role_catalog(&guild_id).await?;
        build_plan(&input.definition, &input.state, &catalog)
    }
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
    let mut lifecycle = Vec::new();

    if let Some(logical_id) = state.pending_creations.iter().next() {
        return Err(ManagementError::InvalidState(format!(
            "Role {logical_id} は作成結果不明のため、同じ定義で状態を確認する必要があります"
        )));
    }
    for logical_id in &state.pending_deletions {
        let Some(role) = definition.roles.get(logical_id) else {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の削除意図が未解決のため、定義を変更できません"
            )));
        };
        if !role.is_absent() {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の削除意図が未解決です"
            )));
        }
    }

    for (logical_id, desired) in &definition.roles {
        if desired.is_absent() {
            if *logical_id == everyone_logical_id() {
                return Err(ManagementError::InvalidDefinition(
                    "@everyone Role は削除できません".to_owned(),
                ));
            }
            let Some(discord_id) = state.roles.get(logical_id).copied() else {
                continue;
            };
            if state.deleted_roles.contains(logical_id) {
                continue;
            }
            if !state.pending_deletions.contains(logical_id) && !actual_roles.contains_key(&discord_id) {
                return Err(ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
                )));
            }
            if let Some(actual) = actual_roles.get(&discord_id)
                && !actual.manageable
            {
                return Err(ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {discord_id} は Bot が管理できないため削除できません"
                )));
            }
            lifecycle.push(RoleLifecycleChange::Delete {
                logical_id: logical_id.clone(),
                discord_id,
            });
            continue;
        }

        if *logical_id == everyone_logical_id() {
            let discord_id = resolve_role_id(logical_id, state)?;
            let actual = actual_roles.get(&discord_id).ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
                ))
            })?;
            let desired_attributes = compose_attributes(desired, &definition.settings_sets.role);
            if desired_attributes.has_non_permission_attributes() {
                return Err(ManagementError::InvalidDefinition(
                    "@everyone Role では権限だけを管理できます".to_owned(),
                ));
            }
            if desired.is_managed() && !actual.manageable {
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
            continue;
        }

        if state.deleted_roles.contains(logical_id) {
            if desired.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "削除済みの Role {logical_id} は参照専用として利用できません"
                )));
            }
            validate_role_creation(logical_id, desired, &definition.settings_sets.role, catalog)?;
            lifecycle.push(RoleLifecycleChange::Create {
                logical_id: logical_id.clone(),
                recreated: true,
            });
            continue;
        }

        let Some(discord_id) = state.roles.get(logical_id).copied() else {
            if desired.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "参照専用 Role {logical_id} の対応がありません"
                )));
            }
            validate_role_creation(logical_id, desired, &definition.settings_sets.role, catalog)?;
            lifecycle.push(RoleLifecycleChange::Create {
                logical_id: logical_id.clone(),
                recreated: false,
            });
            continue;
        };

        if state.pending_deletions.contains(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の削除意図が未解決です"
            )));
        }

        let actual = actual_roles.get(&discord_id).ok_or_else(|| {
            ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
            ))
        })?;
        let desired_attributes = compose_attributes(desired, &definition.settings_sets.role);
        if desired.is_managed() && !actual.manageable {
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

    for (logical_id, discord_id) in &state.roles {
        if *logical_id == everyone_logical_id() || definition.roles.contains_key(logical_id) {
            continue;
        }
        lifecycle.push(RoleLifecycleChange::Release {
            logical_id: logical_id.clone(),
            discord_id: *discord_id,
        });
    }

    Ok(RolePlan { changes, lifecycle })
}

fn validate_role_creation(
    logical_id: &RoleLogicalId,
    definition: &RoleDefinition,
    settings_sets: &BTreeMap<RoleSettingsSetId, RoleAttributes>,
    catalog: &RoleCatalog,
) -> Result<(), ManagementError> {
    let attributes = compose_attributes(definition, settings_sets);
    let Some(name) = attributes.name.as_ref() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "新しい Role {logical_id} には name が必要です"
        )));
    };
    let name = resolve(name, "new role".to_owned());
    if name.is_empty() {
        return Err(ManagementError::InvalidDefinition(format!(
            "新しい Role {logical_id} の name は空にできません"
        )));
    }
    for (permission, value) in &attributes.permissions {
        let default = *catalog
            .default_permissions
            .get(permission)
            .ok_or_else(|| ManagementError::RoleSource(format!("権限 {permission} の Guild 既定値を取得できません")))?;
        let resolved = resolve(value, default);
        if resolved && !catalog.grantable_permissions.contains(permission) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に権限 {permission} を付与できません。Bot 自身がこの権限を持っていません"
            )));
        }
    }
    Ok(())
}

mod apply;

mod bind;

mod model;

pub use model::Color;
use model::*;

#[cfg(test)]
#[path = "service/tests.rs"]
mod tests;
