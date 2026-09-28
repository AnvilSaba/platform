use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    marker::PhantomData,
};

use serde::{
    Deserializer,
    de::{self, MapAccess, Visitor},
};
use serdev::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

use super::{AttributeChange, ManagementError, RoleSnapshot, SCHEMA_VERSION};
use crate::features::discord_management::ids::{
    ChannelId, ChannelLogicalId, ChannelSettingsSetId, GuildId, MemberId, MemberLogicalId, RoleId, RoleLogicalId,
    RoleSettingsSetId,
};

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
    for (logical_id, channel) in &definition.channels {
        validate_channel_settings_sets(logical_id, channel, &definition.settings_sets.channel)?;
    }
    for (logical_id, member) in &definition.members {
        if !matches!(member.mode, RoleMode::Reference) {
            return Err(validation_error(
                "member_must_be_reference",
                format!("Member {logical_id} は参照専用として宣言してください"),
            ));
        }
    }
    Ok(())
}

fn validate_channel_settings_sets(
    logical_id: &ChannelLogicalId,
    channel: &ChannelDefinition,
    settings_sets: &BTreeMap<ChannelSettingsSetId, ChannelSettingsSet>,
) -> Result<(), ValidationError> {
    let Some(value) = channel.attributes.get("settings_sets") else {
        return Ok(());
    };
    let Some(values) = value.as_array() else {
        return Err(validation_error(
            "invalid_channel_settings_sets",
            format!("Channel {logical_id} の settings_sets は配列で指定してください"),
        ));
    };
    let mut seen = BTreeSet::new();
    for value in values {
        let Some(value) = value.as_str() else {
            return Err(validation_error(
                "invalid_channel_settings_set_id",
                format!("Channel {logical_id} の settings_sets に文字列でない値があります"),
            ));
        };
        let settings_set = ChannelSettingsSetId::parse(value).map_err(|error| {
            validation_error(
                "invalid_channel_settings_set_id",
                format!("Channel {logical_id} の settings_sets に不正な ID {value}: {error}"),
            )
        })?;
        if !seen.insert(settings_set.clone()) {
            return Err(validation_error(
                "duplicate_channel_settings_set",
                format!("Channel {logical_id} で設定セット {settings_set} が重複しています"),
            ));
        }
        if !settings_sets.contains_key(&settings_set) {
            return Err(validation_error(
                "unknown_channel_settings_set",
                format!("Channel {logical_id} が未知の設定セット {settings_set} を参照しています"),
            ));
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
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) channels: BTreeMap<ChannelLogicalId, ChannelDefinition>,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) members: BTreeMap<MemberLogicalId, MemberDefinition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) message_sets: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) threads: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) order: Option<toml::Value>,
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
pub(super) struct RoleSettingsSets {
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) role: BTreeMap<RoleSettingsSetId, RoleAttributes>,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) channel: BTreeMap<ChannelSettingsSetId, ChannelSettingsSet>,
}

impl RoleSettingsSets {
    fn is_empty(&self) -> bool {
        self.role.is_empty() && self.channel.is_empty()
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

#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_channel_definition"))]
#[serde(validate = "Validate::validate")]
pub(super) struct ChannelDefinition {
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(super) mode: RoleMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) ensure: Option<Ensure>,
    #[serde(flatten)]
    pub(super) attributes: BTreeMap<String, toml::Value>,
}

impl ChannelDefinition {
    pub(super) fn is_absent(&self) -> bool {
        matches!(self.ensure, Some(Ensure::Absent))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Ensure {
    Present,
    Absent,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(validate = "Validate::validate")]
#[serde(deny_unknown_fields)]
pub(super) struct MemberDefinition {
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(super) mode: RoleMode,
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_channel_settings_set"))]
#[serde(validate = "Validate::validate")]
pub(super) struct ChannelSettingsSet {
    #[serde(flatten)]
    pub(super) attributes: BTreeMap<String, toml::Value>,
}

const CHANNEL_ATTRIBUTE_NAMES: &[&str] = &[
    "type",
    "name",
    "parent",
    "topic",
    "nsfw",
    "slowmode_seconds",
    "default_auto_archive_minutes",
    "default_thread_slowmode_seconds",
    "bitrate",
    "user_limit",
    "rtc_region",
    "video_quality",
    "permissions_sync",
    "overwrites",
    "tags",
    "require_tag",
    "default_reaction",
    "default_sort_order",
    "default_forum_layout",
    "settings_sets",
];

fn validate_channel_definition(channel: &ChannelDefinition) -> Result<(), ValidationError> {
    validate_channel_attributes(&channel.attributes)?;
    if matches!(channel.mode, RoleMode::Reference) && (channel.ensure.is_some() || !channel.attributes.is_empty()) {
        return Err(validation_error(
            "reference_with_managed_attributes",
            "参照専用 Channel には管理属性を指定できません",
        ));
    }
    if matches!(channel.ensure, Some(Ensure::Absent))
        && (matches!(channel.mode, RoleMode::Reference) || !channel.attributes.is_empty())
    {
        return Err(validation_error(
            "absent_with_managed_attributes",
            "削除宣言 Channel には mode や管理属性を指定できません",
        ));
    }
    Ok(())
}

fn validate_channel_settings_set(settings_set: &ChannelSettingsSet) -> Result<(), ValidationError> {
    validate_channel_attributes(&settings_set.attributes)
}

fn validate_channel_attributes(attributes: &BTreeMap<String, toml::Value>) -> Result<(), ValidationError> {
    if let Some(name) = attributes
        .keys()
        .find(|name| !CHANNEL_ATTRIBUTE_NAMES.contains(&name.as_str()))
    {
        return Err(validation_error(
            "unknown_channel_attribute",
            format!("Channel の未知の属性 {name} が指定されています"),
        ));
    }
    Ok(())
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
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_unique_role_mappings")]
    pub(super) roles: BTreeMap<RoleLogicalId, RoleId>,
    #[validate(custom(function = "validate_channel_mappings"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_unique_channel_mappings")]
    pub(super) channels: BTreeMap<ChannelLogicalId, ChannelId>,
    #[validate(custom(function = "validate_member_mappings"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_unique_member_mappings")]
    pub(super) members: BTreeMap<MemberLogicalId, MemberId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) deleted_roles: BTreeSet<RoleLogicalId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) pending_creations: BTreeSet<RoleLogicalId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(super) pending_deletions: BTreeSet<RoleLogicalId>,
}

fn deserialize_unique_mappings<'de, D, LogicalIdType, DiscordIdType>(
    deserializer: D,
    resource_type: &'static str,
) -> Result<BTreeMap<LogicalIdType, DiscordIdType>, D::Error>
where
    D: Deserializer<'de>,
    LogicalIdType: serde::Deserialize<'de> + Clone + Ord + fmt::Display,
    DiscordIdType: serde::Deserialize<'de> + Copy,
{
    struct UniqueMappingsVisitor<LogicalIdType, DiscordIdType> {
        resource_type: &'static str,
        marker: PhantomData<fn() -> (LogicalIdType, DiscordIdType)>,
    }

    impl<'de, LogicalIdType, DiscordIdType> Visitor<'de> for UniqueMappingsVisitor<LogicalIdType, DiscordIdType>
    where
        LogicalIdType: serde::Deserialize<'de> + Clone + Ord + fmt::Display,
        DiscordIdType: serde::Deserialize<'de> + Copy,
    {
        type Value = BTreeMap<LogicalIdType, DiscordIdType>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "重複しない {} 論理 ID と Snowflake の対応表",
                self.resource_type
            )
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut mappings = BTreeMap::new();
            while let Some((logical_id, discord_id)) = map.next_entry::<LogicalIdType, DiscordIdType>()? {
                if mappings.insert(logical_id.clone(), discord_id).is_some() {
                    return Err(de::Error::custom(format!(
                        "{} 論理 ID {logical_id} が重複しています",
                        self.resource_type
                    )));
                }
            }
            Ok(mappings)
        }
    }

    deserializer.deserialize_map(UniqueMappingsVisitor {
        resource_type,
        marker: PhantomData,
    })
}

pub(super) fn deserialize_unique_role_mappings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<RoleLogicalId, RoleId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_unique_mappings(deserializer, "Role")
}

pub(super) fn deserialize_unique_channel_mappings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<ChannelLogicalId, ChannelId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_unique_mappings(deserializer, "Channel")
}

pub(super) fn deserialize_unique_member_mappings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<MemberLogicalId, MemberId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_unique_mappings(deserializer, "Member")
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
    if state.roles.values().any(|role_id| role_id.get() == guild_id.get()) {
        return Err(ManagementError::InvalidState(
            "予約参照 everyone の Role ID は state の別の論理 ID に対応付けできません".to_owned(),
        ));
    }
    Ok(state)
}

pub(super) fn serialize_state(state: &StateFile) -> Result<String, ManagementError> {
    serde_json::to_string_pretty(state)
        .map(|json| format!("{json}\n"))
        .map_err(|error| ManagementError::SerializeState(error.to_string()))
}

pub(super) fn validate_references(definition: &DefinitionFile, state: &StateFile) -> Result<(), ManagementError> {
    for (logical_id, role) in &definition.roles {
        if *logical_id != everyone_logical_id()
            && matches!(role.mode, RoleMode::Reference)
            && !state.roles.contains_key(logical_id)
        {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の対応がありません"
            )));
        }
    }

    for (logical_id, channel) in &definition.channels {
        if channel.is_absent() {
            continue;
        }
        if !state.channels.contains_key(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の対応がありません"
            )));
        }
        validate_channel_references(logical_id, &channel.attributes, definition, state)?;
    }

    for (logical_id, member) in &definition.members {
        if !matches!(member.mode, RoleMode::Reference) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Member {logical_id} は参照専用として宣言してください"
            )));
        }
        if !state.members.contains_key(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Member {logical_id} の対応がありません"
            )));
        }
    }

    for (name, message_set) in &definition.message_sets {
        validate_channel_container_reference("管理メッセージ群", name, message_set, definition, state)?;
    }
    for (name, thread) in &definition.threads {
        validate_channel_container_reference("管理スレッド", name, thread, definition, state)?;
    }

    for (name, settings_set) in &definition.settings_sets.channel {
        validate_channel_references(
            &ChannelLogicalId::parse(format!("settings_set_{name}")).expect("設定セット検証用の論理 ID は常に有効です"),
            &settings_set.attributes,
            definition,
            state,
        )?;
    }

    Ok(())
}

pub(super) fn validate_role_plan_scope(definition: &DefinitionFile) -> Result<(), ManagementError> {
    if let Some(logical_id) = definition
        .channels
        .iter()
        .find_map(|(logical_id, channel)| (!matches!(channel.mode, RoleMode::Reference)).then_some(logical_id))
    {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の属性管理は Channel plan の対象外です。参照専用として宣言してください"
        )));
    }
    if let Some(logical_id) = definition
        .channels
        .iter()
        .find_map(|(logical_id, channel)| channel.is_absent().then_some(logical_id))
    {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の削除管理は Channel plan の対象外です"
        )));
    }
    Ok(())
}

fn validate_channel_container_reference(
    resource_kind: &str,
    resource_name: &str,
    value: &toml::Value,
    definition: &DefinitionFile,
    state: &StateFile,
) -> Result<(), ManagementError> {
    let Some(table) = value.as_table() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} はテーブルで指定してください"
        )));
    };
    if table
        .get("ensure")
        .and_then(toml::Value::as_str)
        .is_some_and(|ensure| ensure == "absent")
    {
        return Ok(());
    }

    let Some(channel) = table.get("channel") else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} の channel がありません"
        )));
    };
    let Some(channel) = channel.as_str() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} の channel は Channel 論理 ID で指定してください"
        )));
    };
    let channel_id = ChannelLogicalId::parse(channel).map_err(|error| {
        ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} の channel {channel} が不正です: {error}"
        ))
    })?;
    require_channel_reference(
        &channel_id,
        definition,
        state,
        &format!("{resource_kind} {resource_name} の投稿先"),
    )
}

fn validate_channel_references(
    channel_id: &ChannelLogicalId,
    attributes: &BTreeMap<String, toml::Value>,
    definition: &DefinitionFile,
    state: &StateFile,
) -> Result<(), ManagementError> {
    if let Some(parent) = attributes.get("parent") {
        if let Some(parent) = parent.as_str() {
            let parent_id = ChannelLogicalId::parse(parent).map_err(|error| {
                ManagementError::InvalidDefinition(format!("Channel {channel_id} の親 {parent} が不正です: {error}"))
            })?;
            require_channel_reference(&parent_id, definition, state, &format!("Channel {channel_id} の親"))?;
        } else if !is_clear_value(parent) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {channel_id} の parent は Channel 論理 ID または clear で指定してください"
            )));
        }
    }

    if let Some(overwrites) = attributes.get("overwrites") {
        let Some(overwrites) = overwrites.as_table() else {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {channel_id} の overwrites はテーブルで指定してください"
            )));
        };
        for subject in overwrites.keys() {
            if subject == "everyone" {
                continue;
            }
            if let Some(logical_id) = subject.strip_prefix("role:") {
                let logical_id = RoleLogicalId::parse(logical_id).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Channel {channel_id} の {subject} が不正です: {error}"))
                })?;
                if logical_id == everyone_logical_id() {
                    continue;
                }
                if !definition.roles.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidDefinition(format!(
                        "Channel {channel_id} の権限対象 Role {logical_id} の宣言がありません"
                    )));
                }
                if !state.roles.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidState(format!(
                        "Channel {channel_id} の権限対象 Role {logical_id} の対応がありません"
                    )));
                }
            } else if let Some(logical_id) = subject.strip_prefix("member:") {
                let logical_id = MemberLogicalId::parse(logical_id).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Channel {channel_id} の {subject} が不正です: {error}"))
                })?;
                if !definition.members.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidDefinition(format!(
                        "Channel {channel_id} の権限対象 Member {logical_id} の宣言がありません"
                    )));
                }
                if !state.members.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidState(format!(
                        "Channel {channel_id} の権限対象 Member {logical_id} の対応がありません"
                    )));
                }
            } else {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {channel_id} の権限対象 {subject} は everyone、role:<論理 ID>、member:<論理 ID> のいずれかで指定してください"
                )));
            }
        }
    }

    Ok(())
}

fn require_channel_reference(
    logical_id: &ChannelLogicalId,
    definition: &DefinitionFile,
    state: &StateFile,
    context: &str,
) -> Result<(), ManagementError> {
    let Some(channel) = definition.channels.get(logical_id) else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{context} Channel {logical_id} の宣言がありません"
        )));
    };
    if channel.is_absent() {
        return Err(ManagementError::InvalidDefinition(format!(
            "{context} Channel {logical_id} は削除宣言です"
        )));
    }
    if !state.channels.contains_key(logical_id) {
        return Err(ManagementError::InvalidState(format!(
            "{context} Channel {logical_id} の対応がありません"
        )));
    }
    Ok(())
}

fn is_clear_value(value: &toml::Value) -> bool {
    value
        .as_table()
        .and_then(|table| table.get("clear"))
        .and_then(toml::Value::as_bool)
        .is_some_and(|clear| clear)
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

    validate_unique_mappings(roles, "Role")
}

pub(super) fn validate_channel_mappings(
    channels: &BTreeMap<ChannelLogicalId, ChannelId>,
) -> Result<(), ValidationError> {
    validate_unique_mappings(channels, "Channel")
}

pub(super) fn validate_member_mappings(members: &BTreeMap<MemberLogicalId, MemberId>) -> Result<(), ValidationError> {
    validate_unique_mappings(members, "Member")
}

fn validate_unique_mappings<LogicalIdType, DiscordIdType>(
    mappings: &BTreeMap<LogicalIdType, DiscordIdType>,
    resource_type: &str,
) -> Result<(), ValidationError>
where
    LogicalIdType: Clone + Ord + fmt::Display,
    DiscordIdType: Copy + Ord + fmt::Display,
{
    let mut seen_ids = BTreeMap::new();
    for (logical_id, discord_id) in mappings {
        if let Some(first_logical_id) = seen_ids.insert(*discord_id, logical_id.clone()) {
            return Err(validation_error(
                "duplicate_snowflake",
                format!(
                    "{resource_type} {first_logical_id} と {logical_id} が同じ Snowflake {discord_id} を参照しています"
                ),
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
