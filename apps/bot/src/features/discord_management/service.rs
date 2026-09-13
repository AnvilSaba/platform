use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::ids::{GuildId, RoleId, RoleLogicalId};

const SCHEMA_VERSION: u32 = 1;

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
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExportFiles {
    pub definition_toml: String,
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
where
    S: RoleSource,
{
    pub fn new(source: S) -> Self {
        Self { source }
    }

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
            if !is_everyone {
                if let Some(existing_id) = mappings.insert(logical_id.clone(), role.id) {
                    return Err(ManagementError::InvalidState(format!(
                        "論理 ID {logical_id} が Role {existing_id} と {} で衝突しています",
                        role.id
                    )));
                }
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
        })
        .map_err(|error| ManagementError::SerializeDefinition(error.to_string()))?;
        let state_json = serde_json::to_string_pretty(&StateFile {
            schema_version: SCHEMA_VERSION,
            guild_id,
            roles: mappings,
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

        let catalog = self.source.role_catalog(&guild_id).await?;
        build_plan(&definition, &state, &catalog)
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
