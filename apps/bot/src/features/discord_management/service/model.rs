use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use serde::{
    Deserializer,
    de::{self, MapAccess, Visitor},
};
use serdev::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

use super::{AttributeChange, ManagementError, RoleSnapshot, SCHEMA_VERSION};
use crate::features::discord_management::ids::{GuildId, RoleId, RoleLogicalId, RoleSettingsSetId};

pub(super) fn compose_attributes(
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

pub(super) fn validate_definition(definition: &DefinitionFile) -> Result<(), ValidationError> {
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

pub(super) fn validate_managed_value<T>(value: &ManagedValue<T>) -> Result<(), ValidationError> {
    if matches!(value, ManagedValue::Default { default: false }) {
        return Err(validation_error(
            "invalid_default",
            "default 指定は true である必要があります",
        ));
    }

    Ok(())
}

pub(super) fn validate_color(value: &ManagedValue<u32>) -> Result<(), ValidationError> {
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

pub(super) fn validate_permissions(permissions: &BTreeMap<String, ManagedValue<bool>>) -> Result<(), ValidationError> {
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

pub(super) fn validate_role_definition(role: &RoleDefinition) -> Result<(), ValidationError> {
    if matches!(role.mode, RoleMode::Reference) && (!role.settings_sets.is_empty() || !role.attributes.is_empty()) {
        return Err(validation_error(
            "reference_with_managed_attributes",
            "参照専用 Role には管理属性を指定できません",
        ));
    }
    if matches!(role.ensure, RoleEnsure::Absent)
        && (matches!(role.mode, RoleMode::Reference) || !role.settings_sets.is_empty() || !role.attributes.is_empty())
    {
        return Err(validation_error(
            "delete_with_managed_attributes",
            "削除する Role には mode、設定セット、管理属性を指定できません",
        ));
    }

    Ok(())
}

pub(super) fn validate_permission_names(
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

pub(super) fn validate_attribute_permission_names(
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

pub(super) fn compare_attributes(
    logical_id: &RoleLogicalId,
    discord_id: &RoleId,
    actual: &RoleSnapshot,
    desired: &RoleAttributes,
    default_permissions: &BTreeMap<String, bool>,
    grantable_permissions: &BTreeSet<String>,
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
        if !*current && desired && !grantable_permissions.contains(permission) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に権限 {permission} を付与できません。Bot 自身がこの権限を持っていません"
            )));
        }
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

pub(super) fn resolve<T: Clone>(value: &ManagedValue<T>, default: T, attribute: &str) -> Result<T, ManagementError> {
    match value {
        ManagedValue::Value(value) => Ok(value.clone()),
        ManagedValue::Default { default: true } => Ok(default),
        ManagedValue::Default { default: false } => Err(ManagementError::InvalidDefinition(format!(
            "{attribute} の default 指定は true である必要があります"
        ))),
    }
}

pub(super) fn push_change(
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
pub(super) struct DefinitionFile {
    #[validate(range(
        min = "SCHEMA_VERSION",
        max = "SCHEMA_VERSION",
        message = "対応していない schema_version です"
    ))]
    pub(super) schema_version: u32,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "RoleSettingsSets::is_empty")]
    pub(super) settings_sets: RoleSettingsSets,
    #[validate(nested)]
    #[serde(default)]
    pub(super) roles: BTreeMap<RoleLogicalId, RoleDefinition>,
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
pub(super) struct RoleSettingsSets {
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) role: BTreeMap<RoleSettingsSetId, RoleAttributes>,
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
pub(super) struct RoleDefinition {
    #[serde(default, skip_serializing_if = "RoleEnsure::is_present")]
    pub(super) ensure: RoleEnsure,
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(super) mode: RoleMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) settings_sets: Vec<RoleSettingsSetId>,
    #[validate(nested)]
    #[serde(flatten)]
    pub(super) attributes: RoleAttributes,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RoleEnsure {
    #[default]
    Present,
    Absent,
}

impl RoleEnsure {
    fn is_present(&self) -> bool {
        matches!(self, Self::Present)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RoleMode {
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
pub(super) struct RoleAttributes {
    #[validate(custom(function = "validate_managed_value"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<ManagedValue<String>>,
    #[validate(custom(function = "validate_color"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) color: Option<ManagedValue<u32>>,
    #[validate(custom(function = "validate_managed_value"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) hoist: Option<ManagedValue<bool>>,
    #[validate(custom(function = "validate_managed_value"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) mentionable: Option<ManagedValue<bool>>,
    #[validate(custom(function = "validate_permissions"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) permissions: BTreeMap<String, ManagedValue<bool>>,
}

impl RoleAttributes {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.color.is_none()
            && self.hoist.is_none()
            && self.mentionable.is_none()
            && self.permissions.is_empty()
    }

    pub(super) fn has_non_permission_attributes(&self) -> bool {
        self.name.is_some() || self.color.is_some() || self.hoist.is_some() || self.mentionable.is_some()
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
pub(super) enum ManagedValue<T> {
    Value(T),
    Default { default: bool },
}

pub(super) fn everyone_logical_id() -> RoleLogicalId {
    RoleLogicalId::parse("everyone").expect("予約済み論理 ID は常に有効です")
}

pub(super) fn resolve_role_id(logical_id: &RoleLogicalId, state: &StateFile) -> Result<RoleId, ManagementError> {
    if *logical_id == everyone_logical_id() {
        return Ok(state
            .guild_id
            .to_string()
            .parse::<RoleId>()
            .expect("Guild Snowflake は Role Snowflake と同じ形式です"));
    }

    state
        .roles
        .get(logical_id)
        .copied()
        .ok_or_else(|| ManagementError::InvalidState(format!("Role {logical_id} の対応がありません")))
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_state"))]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
pub(super) struct StateFile {
    #[validate(range(
        min = "SCHEMA_VERSION",
        max = "SCHEMA_VERSION",
        message = "対応していない schema_version です"
    ))]
    pub(super) schema_version: u32,
    pub(super) guild_id: GuildId,
    #[validate(custom(function = "validate_role_mappings"))]
    #[serde(deserialize_with = "deserialize_unique_role_mappings")]
    pub(super) roles: BTreeMap<RoleLogicalId, RoleId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) deleted_roles: BTreeSet<RoleLogicalId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) pending_creations: BTreeSet<RoleLogicalId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) pending_deletions: BTreeSet<RoleLogicalId>,
}

pub(super) fn deserialize_unique_role_mappings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<RoleLogicalId, RoleId>, D::Error>
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

pub(super) fn deserialize_state_for_guild(contents: &str, guild_id: GuildId) -> Result<StateFile, ManagementError> {
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

pub(super) fn validation_error(code: &'static str, message: impl Into<String>) -> ValidationError {
    ValidationError::new(code).with_message(message.into().into())
}

pub(super) fn validate_role_mappings(roles: &BTreeMap<RoleLogicalId, RoleId>) -> Result<(), ValidationError> {
    if roles.contains_key(&everyone_logical_id()) {
        return Err(validation_error(
            "reserved_everyone_logical_id",
            "予約論理 ID everyone は state に含めず、definition だけで使用してください",
        ));
    }

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

pub(super) fn validate_state(state: &StateFile) -> Result<(), ValidationError> {
    for logical_id in &state.deleted_roles {
        if !state.roles.contains_key(logical_id) {
            return Err(validation_error(
                "deleted_role_without_mapping",
                format!("削除済み Role {logical_id} に対応する Snowflake がありません"),
            ));
        }
        if state.pending_deletions.contains(logical_id) {
            return Err(validation_error(
                "conflicting_role_operation",
                format!("Role {logical_id} に競合する未完了状態があります"),
            ));
        }
    }
    for logical_id in &state.pending_deletions {
        if !state.roles.contains_key(logical_id) || state.deleted_roles.contains(logical_id) {
            return Err(validation_error(
                "invalid_pending_deletion",
                format!("Role {logical_id} の削除意図に対応する active state がありません"),
            ));
        }
    }
    for logical_id in &state.pending_creations {
        if state.roles.contains_key(logical_id) && !state.deleted_roles.contains(logical_id) {
            return Err(validation_error(
                "invalid_pending_creation",
                format!("作成結果不明の Role {logical_id} に Snowflake が設定されています"),
            ));
        }
    }
    Ok(())
}
