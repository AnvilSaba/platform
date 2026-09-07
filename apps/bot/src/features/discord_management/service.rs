use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Mutex, OnceLock},
    time::Instant,
};

use serde::{
    Deserializer,
    de::{self, MapAccess, Visitor},
};
use serdev::{Deserialize, Serialize};
use thiserror::Error;
use validator::{Validate, ValidationError};

use super::ids::{GuildId, RoleId, RoleLogicalId, RoleSettingsSetId};

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
            let logical_id = if let Some(logical_id) = previous_logical_ids.get(&role.id) {
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
            if let Some(existing_id) = mappings.insert(logical_id.clone(), role.id) {
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
                    attributes: RoleAttributes {
                        name: Some(ManagedValue::Value(role.name)),
                        color: Some(ManagedValue::Value(role.color)),
                        hoist: Some(ManagedValue::Value(role.hoist)),
                        mentionable: Some(ManagedValue::Value(role.mentionable)),
                        permissions: role
                            .permissions
                            .into_iter()
                            .map(|(name, value)| (name, ManagedValue::Value(value)))
                            .collect(),
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

static APPLYING_GUILDS: OnceLock<Mutex<BTreeSet<GuildId>>> = OnceLock::new();

struct GuildApplyGuard(GuildId);

impl GuildApplyGuard {
    fn acquire(guild_id: GuildId) -> Option<Self> {
        let mut guilds = APPLYING_GUILDS
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
            .expect("guild apply mutex poisoned");
        if guilds.insert(guild_id) {
            Some(Self(guild_id))
        } else {
            None
        }
    }
}

impl Drop for GuildApplyGuard {
    fn drop(&mut self) {
        APPLYING_GUILDS
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
            .expect("guild apply mutex poisoned")
            .remove(&self.0);
    }
}

impl<S> RoleManagementService<S>
where
    S: RoleTarget,
{
    pub async fn apply_roles(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        processing_deadline: Instant,
    ) -> Result<RoleApplyResult, ManagementError> {
        let state = deserialize_state_for_guild(state_json, guild_id)?;
        let latest_state_json = serialize_state(&state)?;
        let Some(guard) = GuildApplyGuard::acquire(guild_id) else {
            return Ok(RoleApplyResult {
                status: RoleApplyStatus::GuildBusy,
                applied: Vec::new(),
                pending: confirmed_plan.changes.clone(),
                state_json: latest_state_json,
            });
        };

        if Instant::now() >= processing_deadline {
            return Ok(RoleApplyResult {
                status: RoleApplyStatus::DeadlineExceeded,
                applied: Vec::new(),
                pending: confirmed_plan.changes.clone(),
                state_json: latest_state_json,
            });
        }

        let definition: DefinitionFile =
            toml::from_str(definition_toml).map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        let mut catalog = self.source.role_catalog(&guild_id).await?;
        let current_plan = build_plan(&definition, &state, &catalog)?;
        if current_plan != *confirmed_plan {
            return Ok(RoleApplyResult {
                status: RoleApplyStatus::ReplanRequired,
                applied: Vec::new(),
                pending: current_plan.changes,
                state_json: latest_state_json,
            });
        }

        let mut applied = Vec::new();
        let mut pending = confirmed_plan.changes.clone();

        for (logical_id, role_definition) in &definition.roles {
            let Some(role_id) = state.roles.get(logical_id) else {
                continue;
            };
            let role_changes = pending
                .iter()
                .filter(|change| &change.logical_id == logical_id)
                .cloned()
                .collect::<Vec<_>>();
            if role_changes.is_empty() {
                continue;
            }
            if Instant::now() >= processing_deadline {
                return Ok(RoleApplyResult {
                    status: RoleApplyStatus::DeadlineExceeded,
                    applied,
                    pending,
                    state_json: latest_state_json,
                });
            }

            let actual = catalog.roles.iter().find(|role| role.id == *role_id).ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {role_id} が Guild に存在しません"
                ))
            })?;
            let desired = compose_attributes(role_definition, &definition.settings_sets.role);
            let update = build_role_update(actual, &desired, &catalog.default_permissions, logical_id)?;
            if update.is_empty() {
                continue;
            }

            let outcome = match self.source.update_role(&guild_id, role_id, update.clone()).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    return Ok(RoleApplyResult {
                        status: RoleApplyStatus::Failed(error.to_string()),
                        applied,
                        pending,
                        state_json: latest_state_json,
                    });
                }
            };
            catalog = match self.source.role_catalog(&guild_id).await {
                Ok(catalog) => catalog,
                Err(error) => {
                    return Ok(RoleApplyResult {
                        status: RoleApplyStatus::Failed(error.to_string()),
                        applied,
                        pending,
                        state_json: latest_state_json,
                    });
                }
            };
            let matches = catalog
                .roles
                .iter()
                .find(|role| role.id == *role_id)
                .is_some_and(|role| role_matches_update(role, &update));
            if !matches {
                return Ok(RoleApplyResult {
                    status: if outcome == RoleUpdateOutcome::ResponseUnknown {
                        RoleApplyStatus::ResponseUnknown
                    } else {
                        RoleApplyStatus::Failed(format!("Role {logical_id} の更新後の値が希望値と一致しません"))
                    },
                    applied,
                    pending,
                    state_json: latest_state_json,
                });
            }

            applied.extend(role_changes);
            pending.retain(|change| &change.logical_id != logical_id);
        }

        drop(guard);
        Ok(RoleApplyResult {
            status: RoleApplyStatus::Complete,
            applied,
            pending,
            state_json: latest_state_json,
        })
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
        let discord_id = state
            .roles
            .get(logical_id)
            .ok_or_else(|| ManagementError::InvalidState(format!("Role {logical_id} の対応がありません")))?;
        let actual = actual_roles.get(discord_id).ok_or_else(|| {
            ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} が Guild に存在しません"
            ))
        })?;
        if matches!(desired.mode, RoleMode::Managed) && !actual.manageable {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} は Bot が管理できません"
            )));
        }
        let desired_attributes = compose_attributes(desired, &definition.settings_sets.role);
        compare_attributes(
            logical_id,
            discord_id,
            actual,
            &desired_attributes,
            &catalog.default_permissions,
            &mut changes,
        )?;
    }

    Ok(RolePlan { changes })
}

fn serialize_state(state: &StateFile) -> Result<String, ManagementError> {
    serde_json::to_string_pretty(state)
        .map(|json| format!("{json}\n"))
        .map_err(|error| ManagementError::SerializeState(error.to_string()))
}

fn build_role_update(
    actual: &RoleSnapshot,
    desired: &RoleAttributes,
    default_permissions: &BTreeMap<String, bool>,
    logical_id: &RoleLogicalId,
) -> Result<RoleUpdate, ManagementError> {
    let mut update = RoleUpdate::default();
    if let Some(value) = &desired.name {
        let value = resolve(value, "new role".to_owned(), "name")?;
        if value != actual.name {
            update.name = Some(value);
        }
    }
    if let Some(value) = &desired.color {
        let value = resolve(value, 0, "color")?;
        if value != actual.color {
            update.color = Some(value);
        }
    }
    if let Some(value) = &desired.hoist {
        let value = resolve(value, false, "hoist")?;
        if value != actual.hoist {
            update.hoist = Some(value);
        }
    }
    if let Some(value) = &desired.mentionable {
        let value = resolve(value, false, "mentionable")?;
        if value != actual.mentionable {
            update.mentionable = Some(value);
        }
    }
    if !desired.permissions.is_empty() {
        let mut permissions = actual.permissions.clone();
        for (permission, value) in &desired.permissions {
            let current = actual.permissions.get(permission).ok_or_else(|| {
                ManagementError::InvalidDefinition(format!(
                    "Role {logical_id} に未知の権限 {permission} が指定されています"
                ))
            })?;
            let default = *default_permissions.get(permission).ok_or_else(|| {
                ManagementError::RoleSource(format!("権限 {permission} の Guild 既定値を取得できません"))
            })?;
            let value = resolve(value, default, &format!("permissions.{permission}"))?;
            if value != *current {
                permissions.insert(permission.clone(), value);
            }
        }
        if permissions != actual.permissions {
            update.permissions = Some(permissions);
        }
    }
    Ok(update)
}

fn role_matches_update(role: &RoleSnapshot, update: &RoleUpdate) -> bool {
    update.name.as_ref().is_none_or(|name| role.name == *name)
        && update.color.is_none_or(|color| role.color == color)
        && update.hoist.is_none_or(|hoist| role.hoist == hoist)
        && update
            .mentionable
            .is_none_or(|mentionable| role.mentionable == mentionable)
        && update
            .permissions
            .as_ref()
            .is_none_or(|permissions| role.permissions == *permissions)
}

fn compose_attributes(
    definition: &RoleDefinition,
    settings_sets: &BTreeMap<RoleSettingsSetId, RoleAttributes>,
) -> RoleAttributes {
    let mut composed = RoleAttributes::default();
    for name in &definition.settings_sets {
        let attributes = settings_sets
            .get(name)
            .expect("検証済み Role 定義は既知の設定セットだけを参照します");
        composed.merge(attributes);
    }
    composed.merge(&definition.attributes);
    composed
}

fn validate_definition(definition: &DefinitionFile) -> Result<(), ValidationError> {
    for (logical_id, role) in &definition.roles {
        let mut seen = BTreeSet::new();
        for settings_set in &role.settings_sets {
            if !seen.insert(settings_set) {
                return Err(validation_error(
                    "duplicate_settings_set",
                    format!("Role {logical_id} で設定セット {settings_set} が重複しています"),
                ));
            }
            if !definition.settings_sets.role.contains_key(settings_set) {
                return Err(validation_error(
                    "unknown_settings_set",
                    format!("Role {logical_id} が未知の設定セット {settings_set} を参照しています"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_managed_value<T>(value: &ManagedValue<T>) -> Result<(), ValidationError> {
    if matches!(value, ManagedValue::Default { default: false }) {
        return Err(validation_error(
            "invalid_default",
            "default 指定は true である必要があります",
        ));
    }

    Ok(())
}

fn validate_color(value: &ManagedValue<u32>) -> Result<(), ValidationError> {
    validate_managed_value(value)?;
    if let ManagedValue::Value(color) = value
        && *color > 0xFF_FF_FF
    {
        return Err(validation_error(
            "color_out_of_range",
            "color は 0 から 16777215 の範囲で指定してください",
        ));
    }

    Ok(())
}

fn validate_permissions(permissions: &BTreeMap<String, ManagedValue<bool>>) -> Result<(), ValidationError> {
    for (permission, value) in permissions {
        if permission.is_empty()
            || !permission.bytes().enumerate().all(|(index, byte)| {
                (index == 0 && byte.is_ascii_uppercase())
                    || (index > 0 && (byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
            })
        {
            return Err(validation_error(
                "invalid_permission_name",
                format!("不正な権限名 {permission} が指定されています"),
            ));
        }
        validate_managed_value(value)?;
    }

    Ok(())
}

fn validate_role_definition(role: &RoleDefinition) -> Result<(), ValidationError> {
    if matches!(role.mode, RoleMode::Reference) && (!role.settings_sets.is_empty() || !role.attributes.is_empty()) {
        return Err(validation_error(
            "reference_with_managed_attributes",
            "参照専用 Role には管理属性を指定できません",
        ));
    }

    Ok(())
}

fn validate_permission_names(
    definition: &DefinitionFile,
    known_permissions: &BTreeSet<String>,
) -> Result<(), ManagementError> {
    for (name, attributes) in &definition.settings_sets.role {
        validate_attribute_permission_names(attributes, known_permissions, &format!("Role 設定セット {name}"))?;
    }
    for (logical_id, role) in &definition.roles {
        validate_attribute_permission_names(&role.attributes, known_permissions, &format!("Role {logical_id}"))?;
    }
    Ok(())
}

fn validate_attribute_permission_names(
    attributes: &RoleAttributes,
    known_permissions: &BTreeSet<String>,
    context: &str,
) -> Result<(), ManagementError> {
    if let Some(permission) = attributes
        .permissions
        .keys()
        .find(|permission| !known_permissions.contains(*permission))
    {
        return Err(ManagementError::InvalidDefinition(format!(
            "{context} に未知の権限 {permission} が指定されています"
        )));
    }
    Ok(())
}

fn compare_attributes(
    logical_id: &RoleLogicalId,
    discord_id: &RoleId,
    actual: &RoleSnapshot,
    desired: &RoleAttributes,
    default_permissions: &BTreeMap<String, bool>,
    changes: &mut Vec<AttributeChange>,
) -> Result<(), ManagementError> {
    if let Some(value) = &desired.name {
        let desired = resolve(value, "new role".to_owned(), "name")?;
        push_change(changes, logical_id, discord_id, "name", &actual.name, &desired);
    }
    if let Some(value) = &desired.color {
        let desired = resolve(value, 0, "color")?;
        push_change(
            changes,
            logical_id,
            discord_id,
            "color",
            &actual.color.to_string(),
            &desired.to_string(),
        );
    }
    if let Some(value) = &desired.hoist {
        let desired = resolve(value, false, "hoist")?;
        push_change(
            changes,
            logical_id,
            discord_id,
            "hoist",
            &actual.hoist.to_string(),
            &desired.to_string(),
        );
    }
    if let Some(value) = &desired.mentionable {
        let desired = resolve(value, false, "mentionable")?;
        push_change(
            changes,
            logical_id,
            discord_id,
            "mentionable",
            &actual.mentionable.to_string(),
            &desired.to_string(),
        );
    }
    for (permission, value) in &desired.permissions {
        let Some(current) = actual.permissions.get(permission) else {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に未知の権限 {permission} が指定されています"
            )));
        };
        let default = *default_permissions
            .get(permission)
            .ok_or_else(|| ManagementError::RoleSource(format!("権限 {permission} の Guild 既定値を取得できません")))?;
        let desired = resolve(value, default, &format!("permissions.{permission}"))?;
        push_change(
            changes,
            logical_id,
            discord_id,
            &format!("permissions.{permission}"),
            &current.to_string(),
            &desired.to_string(),
        );
    }
    Ok(())
}

fn resolve<T: Clone>(value: &ManagedValue<T>, default: T, attribute: &str) -> Result<T, ManagementError> {
    match value {
        ManagedValue::Value(value) => Ok(value.clone()),
        ManagedValue::Default { default: true } => Ok(default),
        ManagedValue::Default { default: false } => Err(ManagementError::InvalidDefinition(format!(
            "{attribute} の default 指定は true である必要があります"
        ))),
    }
}

fn push_change(
    changes: &mut Vec<AttributeChange>,
    logical_id: &RoleLogicalId,
    discord_id: &RoleId,
    attribute: &str,
    current: &str,
    desired: &str,
) {
    if current != desired {
        changes.push(AttributeChange {
            logical_id: logical_id.clone(),
            discord_id: *discord_id,
            attribute: attribute.to_owned(),
            current: current.to_owned(),
            desired: desired.to_owned(),
        });
    }
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_definition"))]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
struct DefinitionFile {
    #[validate(range(
        min = "SCHEMA_VERSION",
        max = "SCHEMA_VERSION",
        message = "対応していない schema_version です"
    ))]
    schema_version: u32,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "RoleSettingsSets::is_empty")]
    settings_sets: RoleSettingsSets,
    #[validate(nested)]
    #[serde(default)]
    roles: BTreeMap<RoleLogicalId, RoleDefinition>,
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
struct RoleSettingsSets {
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    role: BTreeMap<RoleSettingsSetId, RoleAttributes>,
}

impl RoleSettingsSets {
    fn is_empty(&self) -> bool {
        self.role.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_role_definition"))]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
struct RoleDefinition {
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    mode: RoleMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    settings_sets: Vec<RoleSettingsSetId>,
    #[validate(nested)]
    #[serde(flatten)]
    attributes: RoleAttributes,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RoleMode {
    #[default]
    Managed,
    Reference,
}

impl RoleMode {
    fn is_managed(&self) -> bool {
        matches!(self, Self::Managed)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
struct RoleAttributes {
    #[validate(custom(function = "validate_managed_value"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<ManagedValue<String>>,
    #[validate(custom(function = "validate_color"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<ManagedValue<u32>>,
    #[validate(custom(function = "validate_managed_value"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hoist: Option<ManagedValue<bool>>,
    #[validate(custom(function = "validate_managed_value"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mentionable: Option<ManagedValue<bool>>,
    #[validate(custom(function = "validate_permissions"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    permissions: BTreeMap<String, ManagedValue<bool>>,
}

impl RoleAttributes {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.color.is_none()
            && self.hoist.is_none()
            && self.mentionable.is_none()
            && self.permissions.is_empty()
    }

    fn merge(&mut self, later: &Self) {
        if later.name.is_some() {
            self.name.clone_from(&later.name);
        }
        if later.color.is_some() {
            self.color.clone_from(&later.color);
        }
        if later.hoist.is_some() {
            self.hoist.clone_from(&later.hoist);
        }
        if later.mentionable.is_some() {
            self.mentionable.clone_from(&later.mentionable);
        }
        self.permissions.extend(later.permissions.clone());
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum ManagedValue<T> {
    Value(T),
    Default { default: bool },
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
struct StateFile {
    #[validate(range(
        min = "SCHEMA_VERSION",
        max = "SCHEMA_VERSION",
        message = "対応していない schema_version です"
    ))]
    schema_version: u32,
    guild_id: GuildId,
    #[validate(custom(function = "validate_role_mappings"))]
    #[serde(deserialize_with = "deserialize_unique_role_mappings")]
    roles: BTreeMap<RoleLogicalId, RoleId>,
}

fn deserialize_unique_role_mappings<'de, D>(deserializer: D) -> Result<BTreeMap<RoleLogicalId, RoleId>, D::Error>
where
    D: Deserializer<'de>,
{
    struct UniqueRoleMappingsVisitor;

    impl<'de> Visitor<'de> for UniqueRoleMappingsVisitor {
        type Value = BTreeMap<RoleLogicalId, RoleId>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("重複しない Role 論理 ID と Snowflake の対応表")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut mappings = BTreeMap::new();
            while let Some((logical_id, discord_id)) = map.next_entry::<RoleLogicalId, RoleId>()? {
                if mappings.insert(logical_id.clone(), discord_id).is_some() {
                    return Err(de::Error::custom(format!("Role 論理 ID {logical_id} が重複しています")));
                }
            }
            Ok(mappings)
        }
    }

    deserializer.deserialize_map(UniqueRoleMappingsVisitor)
}

fn deserialize_state_for_guild(contents: &str, guild_id: GuildId) -> Result<StateFile, ManagementError> {
    let state: StateFile =
        serde_json::from_str(contents).map_err(|error| ManagementError::InvalidState(error.to_string()))?;
    if state.guild_id != guild_id {
        return Err(ManagementError::GuildMismatch {
            state_guild_id: state.guild_id,
            actual_guild_id: guild_id,
        });
    }

    Ok(state)
}

fn validation_error(code: &'static str, message: impl Into<String>) -> ValidationError {
    ValidationError::new(code).with_message(message.into().into())
}

fn validate_role_mappings(roles: &BTreeMap<RoleLogicalId, RoleId>) -> Result<(), ValidationError> {
    let mut seen_ids = BTreeMap::new();
    for (logical_id, discord_id) in roles {
        if let Some(first_logical_id) = seen_ids.insert(discord_id, logical_id) {
            return Err(validation_error(
                "duplicate_role_snowflake",
                format!("Role {first_logical_id} と {logical_id} が同じ Snowflake {discord_id} を参照しています"),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::sync::{Semaphore, mpsc};

    struct StatefulFakeRoleSource {
        guild_id: String,
        roles: Vec<RoleSnapshot>,
    }

    impl RoleSource for StatefulFakeRoleSource {
        async fn role_catalog(&self, guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
            if guild_id.to_string() != self.guild_id {
                return Err(ManagementError::RoleSource(format!("Guild {guild_id} は存在しません")));
            }
            let permission_names = self
                .roles
                .iter()
                .flat_map(|role| role.permissions.keys().cloned())
                .collect::<BTreeSet<_>>();
            let default_permissions = self
                .roles
                .iter()
                .find(|role| role.id.get() == guild_id.get())
                .map(|role| role.permissions.clone())
                .unwrap_or_else(|| permission_names.iter().map(|name| (name.clone(), false)).collect());
            Ok(RoleCatalog {
                roles: self.roles.clone(),
                permission_names,
                default_permissions,
            })
        }
    }

    fn role(id: &str, name: &str) -> RoleSnapshot {
        RoleSnapshot {
            id: id.parse().unwrap(),
            manageable: true,
            name: name.to_owned(),
            color: 0,
            hoist: false,
            mentionable: false,
            permissions: BTreeMap::from([("SEND_MESSAGES".to_owned(), false), ("VIEW_CHANNEL".to_owned(), true)]),
        }
    }

    fn literal_string(value: &Option<ManagedValue<String>>) -> Option<&str> {
        match value {
            Some(ManagedValue::Value(value)) => Some(value),
            _ => None,
        }
    }

    fn literal_bool(value: Option<&ManagedValue<bool>>) -> Option<bool> {
        match value {
            Some(ManagedValue::Value(value)) => Some(*value),
            _ => None,
        }
    }

    fn state(guild_id: &str, roles: &str) -> String {
        format!(r#"{{"schema_version":1,"guild_id":"{guild_id}","roles":{roles}}}"#)
    }

    fn logical_id(value: &str) -> RoleLogicalId {
        RoleLogicalId::parse(value).unwrap()
    }

    fn role_id(value: &str) -> RoleId {
        value.parse().unwrap()
    }

    fn guild_id(value: u64) -> GuildId {
        GuildId::new(value)
    }

    #[tokio::test]
    async fn initial_export_uses_snowflakes_for_duplicate_role_names() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営"), role("201", "運営")],
        });

        let files = service.export_roles(guild_id(100), None).await.unwrap();
        let definition: DefinitionFile = toml::from_str(&files.definition_toml).unwrap();
        let state: StateFile = serde_json::from_str(&files.state_json).unwrap();

        assert_eq!(definition.schema_version, SCHEMA_VERSION);
        assert_eq!(
            literal_string(&definition.roles[&logical_id("role_200")].attributes.name),
            Some("運営")
        );
        assert_eq!(
            literal_string(&definition.roles[&logical_id("role_201")].attributes.name),
            Some("運営")
        );
        assert_eq!(
            literal_bool(
                definition.roles[&logical_id("role_200")]
                    .attributes
                    .permissions
                    .get("SEND_MESSAGES"),
            ),
            Some(false)
        );
        assert_eq!(state.guild_id.get(), 100);
        assert_eq!(state.roles[&logical_id("role_200")].get(), 200);
        assert_eq!(state.roles[&logical_id("role_201")].get(), 201);
    }

    #[tokio::test]
    async fn re_export_preserves_logical_ids_from_input_state() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "名称変更後")],
        });
        let previous_state = r#"{
            "schema_version": 1,
            "guild_id": "100",
            "roles": { "moderator": "200" }
        }"#;

        let files = service.export_roles(guild_id(100), Some(previous_state)).await.unwrap();
        let definition: DefinitionFile = toml::from_str(&files.definition_toml).unwrap();
        let state: StateFile = serde_json::from_str(&files.state_json).unwrap();

        assert_eq!(
            literal_string(&definition.roles[&logical_id("moderator")].attributes.name),
            Some("名称変更後")
        );
        assert_eq!(state.roles[&logical_id("moderator")].get(), 200);
        assert!(!definition.roles.contains_key(&logical_id("role_200")));
    }

    #[tokio::test]
    async fn exported_definition_has_no_plan_changes() {
        let source = StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        };
        let service = RoleManagementService::new(source);
        let files = service.export_roles(guild_id(100), None).await.unwrap();

        let plan = service
            .plan_roles(guild_id(100), &files.definition_toml, &files.state_json)
            .await
            .unwrap();

        assert!(plan.changes.is_empty());
    }

    #[tokio::test]
    async fn reference_role_may_target_an_unmanageable_guild_role() {
        let mut external_role = role("200", "外部 Bot");
        external_role.manageable = false;
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![external_role],
        });
        let definition = r#"
            schema_version = 1
            [roles.external_bot]
            mode = "reference"
        "#;

        let plan = service
            .plan_roles(guild_id(100), definition, &state("100", r#"{"external_bot":"200"}"#))
            .await
            .unwrap();

        assert!(plan.changes.is_empty());
    }

    #[tokio::test]
    async fn everyone_role_may_be_used_as_a_reference() {
        let mut everyone = role("100", "@everyone");
        everyone.manageable = false;
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![everyone],
        });
        let definition = r#"
            schema_version = 1
            [roles.everyone]
            mode = "reference"
        "#;

        let plan = service
            .plan_roles(guild_id(100), definition, &state("100", r#"{"everyone":"100"}"#))
            .await
            .unwrap();

        assert!(plan.changes.is_empty());
    }

    #[tokio::test]
    async fn permission_default_uses_everyone_role_value() {
        let mut everyone = role("100", "@everyone");
        everyone.manageable = false;
        let mut moderator = role("200", "運営");
        moderator.permissions.insert("VIEW_CHANNEL".to_owned(), false);
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![everyone, moderator],
        });
        let definition = r#"
            schema_version = 1
            [roles.moderator.permissions]
            VIEW_CHANNEL = { default = true }
        "#;

        let plan = service
            .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
            .await
            .unwrap();

        assert_eq!(plan.changes[0].attribute, "permissions.VIEW_CHANNEL");
        assert_eq!(plan.changes[0].current, "false");
        assert_eq!(plan.changes[0].desired, "true");
    }

    #[tokio::test]
    async fn export_omits_unmanageable_roles_but_keeps_manageable_roles() {
        let mut external_role = role("200", "外部 Bot");
        external_role.manageable = false;
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![external_role, role("201", "運営")],
        });

        let files = service.export_roles(guild_id(100), None).await.unwrap();
        let definition: DefinitionFile = toml::from_str(&files.definition_toml).unwrap();

        assert!(!definition.roles.contains_key(&logical_id("role_200")));
        assert!(definition.roles.contains_key(&logical_id("role_201")));
    }

    #[tokio::test]
    async fn role_settings_set_attributes_are_planned() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let definition = r#"
            schema_version = 1

            [settings_sets.role.staff]
            hoist = true

            [roles.moderator]
            settings_sets = ["staff"]
        "#;
        let state = r#"{
            "schema_version": 1,
            "guild_id": "100",
            "roles": { "moderator": "200" }
        }"#;

        let plan = service.plan_roles(guild_id(100), definition, state).await.unwrap();

        assert_eq!(
            plan.changes,
            vec![AttributeChange {
                logical_id: logical_id("moderator"),
                discord_id: role_id("200"),
                attribute: "hoist".to_owned(),
                current: "false".to_owned(),
                desired: "true".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn direct_attributes_override_later_settings_sets_and_omitted_attributes_are_retained() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let definition = r#"
            schema_version = 1

            [settings_sets.role.first]
            hoist = true
            mentionable = true

            [settings_sets.role.second]
            mentionable = false

            [roles.moderator]
            settings_sets = ["first", "second"]
            mentionable = true
        "#;

        let plan = service
            .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
            .await
            .unwrap();

        assert_eq!(plan.changes.len(), 2);
        assert!(plan.changes.iter().any(|change| change.attribute == "hoist"));
        assert!(plan.changes.iter().any(|change| change.attribute == "mentionable"));
        assert!(!plan.changes.iter().any(|change| change.attribute == "name"));
    }

    #[tokio::test]
    async fn default_specifiers_are_resolved_to_schema_version_values() {
        let mut actual = role("200", "運営");
        actual.color = 0x12_34_56;
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![actual],
        });
        let definition = r#"
            schema_version = 1
            [roles.moderator]
            name = { default = true }
            color = { default = true }
        "#;

        let plan = service
            .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
            .await
            .unwrap();

        assert!(plan.changes.iter().any(|change| {
            change.attribute == "name" && change.current == "運営" && change.desired == "new role"
        }));
        assert!(
            plan.changes
                .iter()
                .any(|change| { change.attribute == "color" && change.current == "1193046" && change.desired == "0" })
        );
    }

    #[tokio::test]
    async fn guild_mismatch_is_reported_before_reading_discord() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: Vec::new(),
        });
        let error = service
            .plan_roles(guild_id(100), "schema_version = 1", &state("999", "{}"))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            ManagementError::GuildMismatch {
                state_guild_id: guild_id(999),
                actual_guild_id: guild_id(100),
            }
        );
    }

    #[tokio::test]
    async fn non_numeric_state_guild_id_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: Vec::new(),
        });
        let error = service
            .plan_roles(guild_id(100), "schema_version = 1", &state("not-a-snowflake", "{}"))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("Guild ID")));
    }

    #[tokio::test]
    async fn duplicate_snowflakes_in_state_are_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let error = service
            .export_roles(
                guild_id(100),
                Some(&state("100", r#"{"moderator":"200","staff":"200"}"#)),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("同じ Snowflake 200")));
    }

    #[tokio::test]
    async fn duplicate_logical_id_keys_in_state_are_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let state = r#"{
            "schema_version": 1,
            "guild_id": "100",
            "roles": { "moderator": "200", "moderator": "201" }
        }"#;

        let error = service.export_roles(guild_id(100), Some(state)).await.unwrap_err();

        assert!(
            matches!(error, ManagementError::InvalidState(message) if message.contains("論理 ID moderator が重複"))
        );
    }

    #[tokio::test]
    async fn generated_logical_id_collision_with_stale_state_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let error = service
            .export_roles(guild_id(100), Some(&state("100", r#"{"role_200":"999"}"#)))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("論理 ID role_200")));
    }

    #[tokio::test]
    async fn unknown_definition_keys_are_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let definition = r#"
            schema_version = 1
            [roles.moderator]
            unknown = true
        "#;

        let error = service
            .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidDefinition(_)));
    }

    #[tokio::test]
    async fn invalid_unreferenced_settings_set_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let definition = r#"
            schema_version = 1
            [settings_sets.role.unused]
            color = 16777216
        "#;

        let error = service
            .plan_roles(guild_id(100), definition, &state("100", "{}"))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("color")));
    }

    #[tokio::test]
    async fn false_default_in_unreferenced_settings_set_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let definition = r#"
            schema_version = 1
            [settings_sets.role.unused]
            hoist = { default = false }
        "#;

        let error = service
            .plan_roles(guild_id(100), definition, &state("100", "{}"))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("default")));
    }

    #[tokio::test]
    async fn unknown_permission_in_unreferenced_settings_set_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: vec![role("200", "運営")],
        });
        let definition = r#"
            schema_version = 1
            [settings_sets.role.unused.permissions]
            NOT_A_DISCORD_PERMISSION = true
        "#;

        let error = service
            .plan_roles(guild_id(100), definition, &state("100", "{}"))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未知の権限")));
    }

    #[tokio::test]
    async fn unsupported_definition_version_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: Vec::new(),
        });

        let error = service
            .plan_roles(guild_id(100), "schema_version = 2", &state("100", "{}"))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("schema_version")));
    }

    #[test]
    fn editor_sample_matches_the_supported_definition_shape() {
        let sample = include_str!("../../../../../docs/examples/discord-role-management.toml");
        toml::from_str::<DefinitionFile>(sample).unwrap();
    }

    #[tokio::test]
    async fn unknown_settings_set_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: Vec::new(),
        });
        let definition = r#"
            schema_version = 1
            [roles.moderator]
            settings_sets = ["missing"]
        "#;

        let error = service
            .plan_roles(guild_id(100), definition, &state("100", "{}"))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未知の設定セット")));
    }

    #[tokio::test]
    async fn reference_role_with_managed_attributes_is_reported() {
        let service = RoleManagementService::new(StatefulFakeRoleSource {
            guild_id: "100".to_owned(),
            roles: Vec::new(),
        });
        let definition = r#"
            schema_version = 1
            [roles.external]
            mode = "reference"
            hoist = true
        "#;

        let error = service
            .plan_roles(guild_id(100), definition, &state("100", "{}"))
            .await
            .unwrap_err();

        assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("参照専用 Role")));
    }

    #[derive(Clone)]
    struct ApplyingFakeRoleSource {
        catalog: Arc<Mutex<RoleCatalog>>,
        updates: Arc<Mutex<Vec<RoleUpdate>>>,
        outcome: RoleUpdateOutcome,
        apply_update: bool,
    }

    impl RoleSource for ApplyingFakeRoleSource {
        async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
            Ok(self.catalog.lock().unwrap().clone())
        }
    }

    impl RoleTarget for ApplyingFakeRoleSource {
        async fn update_role(
            &self,
            _guild_id: &GuildId,
            role_id: &RoleId,
            update: RoleUpdate,
        ) -> Result<RoleUpdateOutcome, ManagementError> {
            self.updates.lock().unwrap().push(update.clone());
            if self.apply_update {
                let mut catalog = self.catalog.lock().unwrap();
                let role = catalog.roles.iter_mut().find(|role| role.id == *role_id).unwrap();
                update.apply_to(role);
            }
            Ok(self.outcome)
        }
    }

    #[tokio::test]
    async fn apply_updates_only_explicit_attributes_and_preserves_omitted_permissions() {
        let mut moderator = role("200", "運営");
        moderator.permissions =
            BTreeMap::from([("VIEW_CHANNEL".to_owned(), false), ("MANAGE_MESSAGES".to_owned(), true)]);
        let source = ApplyingFakeRoleSource {
            catalog: Arc::new(Mutex::new(RoleCatalog {
                roles: vec![moderator],
                permission_names: BTreeSet::from(["VIEW_CHANNEL".to_owned(), "MANAGE_MESSAGES".to_owned()]),
                default_permissions: BTreeMap::from([
                    ("VIEW_CHANNEL".to_owned(), false),
                    ("MANAGE_MESSAGES".to_owned(), false),
                ]),
            })),
            updates: Arc::new(Mutex::new(Vec::new())),
            outcome: RoleUpdateOutcome::Applied,
            apply_update: true,
        };
        let service = RoleManagementService::new(source.clone());
        let definition = r#"
            schema_version = 1
            [roles.moderator]
            name = "モデレーター"
            [roles.moderator.permissions]
            VIEW_CHANNEL = true
        "#;
        let state = state("100", r#"{"moderator":"200"}"#);
        let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

        let result = service
            .apply_roles(
                guild_id(100),
                definition,
                &state,
                &plan,
                std::time::Instant::now() + Duration::from_secs(60),
            )
            .await
            .unwrap();

        assert_eq!(result.status, RoleApplyStatus::Complete);
        assert_eq!(result.applied, plan.changes);
        assert!(result.pending.is_empty());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap(),
            serde_json::json!({
                "schema_version": 1,
                "guild_id": "100",
                "roles": { "moderator": "200" }
            })
        );
        let updates = source.updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].name.as_deref(), Some("モデレーター"));
        assert_eq!(
            updates[0].permissions,
            Some(BTreeMap::from([
                ("MANAGE_MESSAGES".to_owned(), true),
                ("VIEW_CHANNEL".to_owned(), true),
            ]))
        );
    }

    #[tokio::test]
    async fn apply_requires_a_new_plan_when_managed_attributes_changed_after_confirmation() {
        let source = ApplyingFakeRoleSource {
            catalog: Arc::new(Mutex::new(RoleCatalog {
                roles: vec![role("200", "運営")],
                permission_names: BTreeSet::new(),
                default_permissions: BTreeMap::new(),
            })),
            updates: Arc::new(Mutex::new(Vec::new())),
            outcome: RoleUpdateOutcome::Applied,
            apply_update: true,
        };
        let service = RoleManagementService::new(source.clone());
        let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
        let state = state("100", r#"{"moderator":"200"}"#);
        let confirmed_plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();
        source.catalog.lock().unwrap().roles[0].name = "外部変更".to_owned();

        let result = service
            .apply_roles(
                guild_id(100),
                definition,
                &state,
                &confirmed_plan,
                Instant::now() + Duration::from_secs(60),
            )
            .await
            .unwrap();

        assert_eq!(result.status, RoleApplyStatus::ReplanRequired);
        assert!(result.applied.is_empty());
        assert!(source.updates.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unknown_update_response_stops_when_refetched_value_does_not_match() {
        let source = ApplyingFakeRoleSource {
            catalog: Arc::new(Mutex::new(RoleCatalog {
                roles: vec![role("200", "運営")],
                permission_names: BTreeSet::new(),
                default_permissions: BTreeMap::new(),
            })),
            updates: Arc::new(Mutex::new(Vec::new())),
            outcome: RoleUpdateOutcome::ResponseUnknown,
            apply_update: false,
        };
        let service = RoleManagementService::new(source);
        let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
        let state = state("100", r#"{"moderator":"200"}"#);
        let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

        let result = service
            .apply_roles(
                guild_id(100),
                definition,
                &state,
                &plan,
                Instant::now() + Duration::from_secs(60),
            )
            .await
            .unwrap();

        assert_eq!(result.status, RoleApplyStatus::ResponseUnknown);
        assert!(result.applied.is_empty());
        assert_eq!(result.pending, plan.changes);
    }

    #[tokio::test]
    async fn expired_processing_budget_starts_no_updates_and_returns_latest_state() {
        let source = ApplyingFakeRoleSource {
            catalog: Arc::new(Mutex::new(RoleCatalog {
                roles: vec![role("200", "運営")],
                permission_names: BTreeSet::new(),
                default_permissions: BTreeMap::new(),
            })),
            updates: Arc::new(Mutex::new(Vec::new())),
            outcome: RoleUpdateOutcome::Applied,
            apply_update: true,
        };
        let service = RoleManagementService::new(source.clone());
        let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
        let state = state("100", r#"{"moderator":"200"}"#);
        let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

        let result = service
            .apply_roles(guild_id(100), definition, &state, &plan, Instant::now())
            .await
            .unwrap();

        assert_eq!(result.status, RoleApplyStatus::DeadlineExceeded);
        assert!(result.applied.is_empty());
        assert_eq!(result.pending, plan.changes);
        assert!(source.updates.lock().unwrap().is_empty());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap()["guild_id"],
            "100"
        );
    }

    #[derive(Clone)]
    struct FailsOnSecondUpdate {
        catalog: Arc<Mutex<RoleCatalog>>,
        calls: Arc<AtomicUsize>,
    }

    impl RoleSource for FailsOnSecondUpdate {
        async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
            Ok(self.catalog.lock().unwrap().clone())
        }
    }

    impl RoleTarget for FailsOnSecondUpdate {
        async fn update_role(
            &self,
            _guild_id: &GuildId,
            role_id: &RoleId,
            update: RoleUpdate,
        ) -> Result<RoleUpdateOutcome, ManagementError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err(ManagementError::RoleSource("injected failure".to_owned()));
            }
            let mut catalog = self.catalog.lock().unwrap();
            update.apply_to(catalog.roles.iter_mut().find(|role| role.id == *role_id).unwrap());
            Ok(RoleUpdateOutcome::Applied)
        }
    }

    #[tokio::test]
    async fn apply_stops_at_first_failure_and_reports_successful_and_pending_changes() {
        let source = FailsOnSecondUpdate {
            catalog: Arc::new(Mutex::new(RoleCatalog {
                roles: vec![role("200", "A"), role("201", "B")],
                permission_names: BTreeSet::new(),
                default_permissions: BTreeMap::new(),
            })),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let service = RoleManagementService::new(source);
        let definition = "schema_version = 1\n[roles.a]\nname = \"new A\"\n[roles.b]\nname = \"new B\"\n";
        let state = state("100", r#"{"a":"200","b":"201"}"#);
        let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

        let result = service
            .apply_roles(
                guild_id(100),
                definition,
                &state,
                &plan,
                Instant::now() + Duration::from_secs(60),
            )
            .await
            .unwrap();

        assert!(matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("injected failure")));
        assert_eq!(
            result
                .applied
                .iter()
                .map(|change| change.logical_id.to_string())
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert_eq!(
            result
                .pending
                .iter()
                .map(|change| change.logical_id.to_string())
                .collect::<Vec<_>>(),
            ["b"]
        );
    }

    #[derive(Clone)]
    struct BlockingRoleTarget {
        catalog: Arc<Mutex<RoleCatalog>>,
        started: mpsc::UnboundedSender<()>,
        release: Arc<Semaphore>,
    }

    impl RoleSource for BlockingRoleTarget {
        async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
            Ok(self.catalog.lock().unwrap().clone())
        }
    }

    impl RoleTarget for BlockingRoleTarget {
        async fn update_role(
            &self,
            _guild_id: &GuildId,
            role_id: &RoleId,
            update: RoleUpdate,
        ) -> Result<RoleUpdateOutcome, ManagementError> {
            self.started.send(()).unwrap();
            self.release.acquire().await.unwrap().forget();
            let mut catalog = self.catalog.lock().unwrap();
            update.apply_to(catalog.roles.iter_mut().find(|role| role.id == *role_id).unwrap());
            Ok(RoleUpdateOutcome::Applied)
        }
    }

    #[tokio::test]
    async fn concurrent_apply_for_the_same_guild_is_rejected_without_waiting() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let source = BlockingRoleTarget {
            catalog: Arc::new(Mutex::new(RoleCatalog {
                roles: vec![role("200", "運営")],
                permission_names: BTreeSet::new(),
                default_permissions: BTreeMap::new(),
            })),
            started: started_tx,
            release: Arc::new(Semaphore::new(0)),
        };
        let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
        let state = state("100", r#"{"moderator":"200"}"#);
        let plan = RoleManagementService::new(source.clone())
            .plan_roles(guild_id(100), definition, &state)
            .await
            .unwrap();
        let first_source = source.clone();
        let first_plan = plan.clone();
        let first_state = state.clone();
        let first = tokio::spawn(async move {
            RoleManagementService::new(first_source)
                .apply_roles(
                    guild_id(100),
                    definition,
                    &first_state,
                    &first_plan,
                    Instant::now() + Duration::from_secs(60),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
            .await
            .expect("first apply did not reach the update")
            .unwrap();

        let second = tokio::time::timeout(
            Duration::from_secs(1),
            RoleManagementService::new(source.clone()).apply_roles(
                guild_id(100),
                definition,
                &state,
                &plan,
                Instant::now() + Duration::from_secs(60),
            ),
        )
        .await
        .expect("second apply waited instead of being rejected")
        .unwrap();
        assert_eq!(second.status, RoleApplyStatus::GuildBusy);

        source.release.add_permits(1);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), first)
                .await
                .expect("first apply did not finish after release")
                .unwrap()
                .unwrap()
                .status,
            RoleApplyStatus::Complete
        );
    }
}
