use std::collections::{BTreeMap, BTreeSet};

use super::{KnownPermission, PermissionName, PermissionVocabulary};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ChannelKind {
    Category,
    Text,
    /// Discord 上存在但構成管理ではまだ属性を管理しない Channel 種別です。
    ///
    /// catalog から除外すると、管理対象 Category の配下にある Voice/Forum 等を
    /// 削除前検証で見落とすため、読み取り時だけこの種別で保持します。
    Unsupported,
}

impl ChannelKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Category => "category",
            Self::Text => "text",
            Self::Unsupported => "unsupported",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "category" => Some(Self::Category),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChannelValue<T> {
    Value(T),
    Default,
    Clear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverwriteValue {
    Allow,
    Deny,
    Clear,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ChannelAttributes {
    pub(crate) kind: Option<ChannelKind>,
    pub(crate) name: Option<ChannelValue<String>>,
    pub(crate) parent: Option<ChannelValue<crate::features::discord_management::ids::ChannelLogicalId>>,
    pub(crate) topic: Option<ChannelValue<String>>,
    pub(crate) nsfw: Option<ChannelValue<bool>>,
    pub(crate) slowmode_seconds: Option<ChannelValue<u16>>,
    pub(crate) default_auto_archive_minutes: Option<ChannelValue<u16>>,
    pub(crate) default_thread_slowmode_seconds: Option<ChannelValue<u16>>,
    pub(crate) overwrites: BTreeMap<String, BTreeMap<KnownPermission, OverwriteValue>>,
}

impl ChannelAttributes {
    pub(crate) fn parse(
        logical_id: &crate::features::discord_management::ids::ChannelLogicalId,
        mut attributes: BTreeMap<String, toml::Value>,
        vocabulary: &PermissionVocabulary,
    ) -> Result<Self, ManagementError> {
        let kind = attributes
            .remove("type")
            .map(|value| {
                value.as_str().and_then(ChannelKind::parse).ok_or_else(|| {
                    ManagementError::InvalidDefinition(format!(
                        "Channel {logical_id} の type は category または text で指定してください"
                    ))
                })
            })
            .transpose()?;
        let name = attributes
            .remove("name")
            .map(|value| parse_string_value(value, logical_id, "name", false))
            .transpose()?;
        let parent = attributes
            .remove("parent")
            .map(|value| parse_logical_id_value(value, logical_id, "parent"))
            .transpose()?;
        let topic = attributes
            .remove("topic")
            .map(|value| parse_string_value(value, logical_id, "topic", true))
            .transpose()?;
        let nsfw = attributes
            .remove("nsfw")
            .map(|value| parse_bool_value(value, logical_id, "nsfw"))
            .transpose()?;
        let slowmode_seconds = attributes
            .remove("slowmode_seconds")
            .map(|value| parse_u16_value(value, logical_id, "slowmode_seconds", true))
            .transpose()?;
        let default_auto_archive_minutes = attributes
            .remove("default_auto_archive_minutes")
            .map(|value| parse_auto_archive_value(value, logical_id))
            .transpose()?;
        let default_thread_slowmode_seconds = attributes
            .remove("default_thread_slowmode_seconds")
            .map(|value| parse_u16_value(value, logical_id, "default_thread_slowmode_seconds", true))
            .transpose()?;
        let overwrites = attributes
            .remove("overwrites")
            .map(|value| parse_overwrites(value, logical_id, vocabulary))
            .transpose()?
            .unwrap_or_default();

        if let Some((name, _)) = attributes.into_iter().next() {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の未知の属性 {name} が指定されています"
            )));
        }

        let parsed = Self {
            kind,
            name,
            parent,
            topic,
            nsfw,
            slowmode_seconds,
            default_auto_archive_minutes,
            default_thread_slowmode_seconds,
            overwrites,
        };
        parsed.validate_for_kind(logical_id).map(|()| parsed)
    }

    pub(crate) fn merge(&mut self, later: &Self) {
        if later.kind.is_some() {
            self.kind = later.kind;
        }
        if later.name.is_some() {
            self.name.clone_from(&later.name);
        }
        if later.parent.is_some() {
            self.parent.clone_from(&later.parent);
        }
        if later.topic.is_some() {
            self.topic.clone_from(&later.topic);
        }
        if later.nsfw.is_some() {
            self.nsfw.clone_from(&later.nsfw);
        }
        if later.slowmode_seconds.is_some() {
            self.slowmode_seconds.clone_from(&later.slowmode_seconds);
        }
        if later.default_auto_archive_minutes.is_some() {
            self.default_auto_archive_minutes
                .clone_from(&later.default_auto_archive_minutes);
        }
        if later.default_thread_slowmode_seconds.is_some() {
            self.default_thread_slowmode_seconds
                .clone_from(&later.default_thread_slowmode_seconds);
        }
        for (subject, permissions) in &later.overwrites {
            self.overwrites
                .entry(subject.clone())
                .or_default()
                .extend(permissions.clone());
        }
    }

    pub(crate) fn validate_for_kind(
        &self,
        logical_id: &crate::features::discord_management::ids::ChannelLogicalId,
    ) -> Result<(), ManagementError> {
        let Some(kind) = self.kind else {
            return Ok(());
        };
        if kind == ChannelKind::Category
            && (self.parent.is_some()
                || self.topic.is_some()
                || self.slowmode_seconds.is_some()
                || self.default_auto_archive_minutes.is_some()
                || self.default_thread_slowmode_seconds.is_some())
        {
            return Err(ManagementError::InvalidDefinition(format!(
                "Category {logical_id} には Text 専用属性を指定できません"
            )));
        }
        if kind == ChannelKind::Text
            && self
                .parent
                .as_ref()
                .is_some_and(|value| matches!(value, ChannelValue::Default))
        {
            return Err(ManagementError::InvalidDefinition(format!(
                "Text Channel {logical_id} の parent に default は指定できません"
            )));
        }
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.kind.is_none()
            && self.name.is_none()
            && self.parent.is_none()
            && self.topic.is_none()
            && self.nsfw.is_none()
            && self.slowmode_seconds.is_none()
            && self.default_auto_archive_minutes.is_none()
            && self.default_thread_slowmode_seconds.is_none()
            && self.overwrites.is_empty()
    }
}

fn parse_marker(
    value: &toml::Value,
    logical_id: &impl std::fmt::Display,
    attribute: &str,
) -> Result<Option<bool>, ManagementError> {
    let Some(table) = value.as_table() else {
        return Ok(None);
    };
    if table.len() != 1 {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は具体値、default = true、clear = true のいずれかで指定してください"
        )));
    }
    if let Some(default) = table.get("default") {
        return match default.as_bool() {
            Some(true) => Ok(Some(false)),
            _ => Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の {attribute}.default は true である必要があります"
            ))),
        };
    }
    if let Some(clear) = table.get("clear") {
        return match clear.as_bool() {
            Some(true) => Ok(Some(true)),
            _ => Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の {attribute}.clear は true である必要があります"
            ))),
        };
    }
    Err(ManagementError::InvalidDefinition(format!(
        "Channel {logical_id} の {attribute} マーカーが不正です"
    )))
}

fn parse_string_value(
    value: toml::Value,
    logical_id: &impl std::fmt::Display,
    attribute: &str,
    allow_clear: bool,
) -> Result<ChannelValue<String>, ManagementError> {
    if let Some(marker) = parse_marker(&value, logical_id, attribute)? {
        return match (marker, allow_clear) {
            (false, _) => Ok(ChannelValue::Default),
            (true, true) => Ok(ChannelValue::Clear),
            (true, false) => Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の {attribute} は解除できません"
            ))),
        };
    }
    let value = value.as_str().ok_or_else(|| {
        ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は文字列で指定してください"
        ))
    })?;
    let limit = match attribute {
        "name" => Some(100),
        "topic" => Some(1024),
        _ => None,
    };
    if attribute == "name" && value.is_empty() {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の name は1文字以上で指定してください"
        )));
    }
    if limit.is_some_and(|limit| value.chars().count() > limit) {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は{}文字以内で指定してください",
            limit.expect("limit がある属性だけ長さを検証します")
        )));
    }
    Ok(ChannelValue::Value(value.to_owned()))
}

fn parse_logical_id_value(
    value: toml::Value,
    logical_id: &impl std::fmt::Display,
    attribute: &str,
) -> Result<ChannelValue<crate::features::discord_management::ids::ChannelLogicalId>, ManagementError> {
    if let Some(marker) = parse_marker(&value, logical_id, attribute)? {
        return Ok(if marker {
            ChannelValue::Clear
        } else {
            ChannelValue::Default
        });
    }
    let Some(value) = value.as_str() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は Channel 論理 ID または clear で指定してください"
        )));
    };
    let value = crate::features::discord_management::ids::ChannelLogicalId::parse(value).map_err(|error| {
        ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} {value} が不正です: {error}"
        ))
    })?;
    Ok(ChannelValue::Value(value))
}

fn parse_bool_value(
    value: toml::Value,
    logical_id: &impl std::fmt::Display,
    attribute: &str,
) -> Result<ChannelValue<bool>, ManagementError> {
    if let Some(marker) = parse_marker(&value, logical_id, attribute)? {
        return if marker {
            Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の {attribute} は解除できません"
            )))
        } else {
            Ok(ChannelValue::Default)
        };
    }
    value.as_bool().map(ChannelValue::Value).ok_or_else(|| {
        ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は真偽値で指定してください"
        ))
    })
}

fn parse_u16_value(
    value: toml::Value,
    logical_id: &impl std::fmt::Display,
    attribute: &str,
    allow_clear: bool,
) -> Result<ChannelValue<u16>, ManagementError> {
    if let Some(marker) = parse_marker(&value, logical_id, attribute)? {
        return match (marker, allow_clear) {
            (false, _) => Ok(ChannelValue::Default),
            (true, true) => Ok(ChannelValue::Clear),
            (true, false) => Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の {attribute} は解除できません"
            ))),
        };
    }
    let value = value.as_integer().ok_or_else(|| {
        ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は0以上の整数で指定してください"
        ))
    })?;
    let value = u16::try_from(value).map_err(|_| {
        ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は0から65535の範囲で指定してください"
        ))
    })?;
    if matches!(attribute, "slowmode_seconds" | "default_thread_slowmode_seconds") && value > 21_600 {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は0から21600の範囲で指定してください"
        )));
    }
    Ok(ChannelValue::Value(value))
}

fn parse_auto_archive_value(
    value: toml::Value,
    logical_id: &impl std::fmt::Display,
) -> Result<ChannelValue<u16>, ManagementError> {
    if let Some(marker) = parse_marker(&value, logical_id, "default_auto_archive_minutes")? {
        return Ok(if marker {
            ChannelValue::Clear
        } else {
            ChannelValue::Default
        });
    }
    let value = value.as_integer().ok_or_else(|| {
        ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の default_auto_archive_minutes は60、1440、4320、10080のいずれかで指定してください"
        ))
    })?;
    if matches!(value, 60 | 1440 | 4320 | 10080) {
        Ok(ChannelValue::Value(value as u16))
    } else {
        Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の default_auto_archive_minutes は60、1440、4320、10080のいずれかで指定してください"
        )))
    }
}

fn parse_overwrites(
    value: toml::Value,
    logical_id: &impl std::fmt::Display,
    vocabulary: &PermissionVocabulary,
) -> Result<BTreeMap<String, BTreeMap<KnownPermission, OverwriteValue>>, ManagementError> {
    let Some(subjects) = value.as_table() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の overwrites はテーブルで指定してください"
        )));
    };
    let mut result = BTreeMap::new();
    for (subject, permissions) in subjects {
        if subject != "everyone" && !(subject.starts_with("role:") || subject.starts_with("member:")) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の権限対象 {subject} は everyone、role:<論理 ID>、member:<論理 ID> のいずれかで指定してください"
            )));
        }
        if subject.starts_with("role:") {
            crate::features::discord_management::ids::RoleLogicalId::parse(&subject[5..]).map_err(|error| {
                ManagementError::InvalidDefinition(format!("Channel {logical_id} の {subject} が不正です: {error}"))
            })?;
        }
        if subject.starts_with("member:") {
            crate::features::discord_management::ids::MemberLogicalId::parse(&subject[7..]).map_err(|error| {
                ManagementError::InvalidDefinition(format!("Channel {logical_id} の {subject} が不正です: {error}"))
            })?;
        }
        let permissions = permissions.as_table().ok_or_else(|| {
            ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の {subject} は権限テーブルで指定してください"
            ))
        })?;
        let mut parsed = BTreeMap::new();
        for (name, value) in permissions {
            let permission = PermissionName::parse(name.clone()).map_err(ManagementError::InvalidDefinition)?;
            let permission = vocabulary.resolve(&permission).ok_or_else(|| {
                ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} の {subject} に未知の権限 {name} が指定されています"
                ))
            })?;
            let action = value
                .as_str()
                .and_then(|value| match value {
                    "allow" => Some(OverwriteValue::Allow),
                    "deny" => Some(OverwriteValue::Deny),
                    "clear" => Some(OverwriteValue::Clear),
                    _ => None,
                })
                .ok_or_else(|| {
                    ManagementError::InvalidDefinition(format!(
                        "Channel {logical_id} の {subject}.{name} は allow、deny、clear のいずれかで指定してください"
                    ))
                })?;
            parsed.insert(permission, action);
        }
        result.insert(subject.clone(), parsed);
    }
    Ok(result)
}

use crate::features::discord_management::domain::ManagementError;

#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_channel_definition"))]
pub(crate) struct RawChannelDefinition {
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(crate) mode: RoleMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ensure: Option<Ensure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) settings_sets: Vec<ChannelSettingsSetId>,
    #[serde(flatten)]
    pub(crate) attributes: BTreeMap<String, toml::Value>,
}

#[derive(Debug)]
pub(crate) enum ChannelDefinition {
    Managed {
        #[cfg_attr(not(test), allow(dead_code))]
        settings_sets: Vec<ChannelSettingsSetId>,
        attributes: ChannelAttributes,
    },
    Reference,
    Absent,
}

impl ChannelDefinition {
    pub(super) fn parse(
        logical_id: ChannelLogicalId,
        raw: RawChannelDefinition,
        settings_sets: &BTreeMap<ChannelSettingsSetId, ChannelSettingsSet>,
        vocabulary: &PermissionVocabulary,
    ) -> Result<Self, ManagementError> {
        let mut seen = BTreeSet::new();
        for settings_set in &raw.settings_sets {
            if !seen.insert(settings_set) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} で設定セット {settings_set} が重複しています"
                )));
            }
            if !settings_sets.contains_key(settings_set) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} が未知の設定セット {settings_set} を参照しています"
                )));
            }
        }

        let mut attributes = ChannelAttributes::default();
        for settings_set in &raw.settings_sets {
            let settings = settings_sets
                .get(settings_set)
                .expect("検証済み Channel 定義は既知の設定セットだけを参照します");
            let settings = ChannelAttributes::parse(&logical_id, settings.attributes.clone(), vocabulary)?;
            attributes.merge(&settings);
        }
        let direct_attributes = ChannelAttributes::parse(&logical_id, raw.attributes, vocabulary)?;
        attributes.merge(&direct_attributes);
        attributes.validate_for_kind(&logical_id)?;
        match (raw.ensure, raw.mode) {
            (Some(Ensure::Absent), RoleMode::Managed) if raw.settings_sets.is_empty() && attributes.is_empty() => {
                Ok(Self::Absent)
            }
            (Some(Ensure::Absent), _) => Err(ManagementError::InvalidDefinition(format!(
                "削除宣言 Channel {logical_id} には mode や管理属性を指定できません"
            ))),
            (None, RoleMode::Reference) if raw.settings_sets.is_empty() && attributes.is_empty() => Ok(Self::Reference),
            (_, RoleMode::Reference) => Err(ManagementError::InvalidDefinition(format!(
                "参照専用 Channel {logical_id} には ensure や管理属性を指定できません"
            ))),
            (None | Some(Ensure::Present), RoleMode::Managed) => Ok(Self::Managed {
                settings_sets: raw.settings_sets,
                attributes,
            }),
        }
    }

    pub(crate) fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }

    pub(crate) fn is_reference(&self) -> bool {
        matches!(self, Self::Reference)
    }

    pub(crate) fn is_managed(&self) -> bool {
        matches!(self, Self::Managed { .. })
    }

    pub(crate) fn attributes(&self) -> &ChannelAttributes {
        static EMPTY: std::sync::OnceLock<ChannelAttributes> = std::sync::OnceLock::new();
        match self {
            Self::Managed { attributes, .. } => attributes,
            Self::Reference | Self::Absent => EMPTY.get_or_init(ChannelAttributes::default),
        }
    }

    #[cfg(test)]
    pub(crate) fn settings_sets(&self) -> &[ChannelSettingsSetId] {
        match self {
            Self::Managed { settings_sets, .. } => settings_sets,
            Self::Reference | Self::Absent => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Ensure {
    Present,
    Absent,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemberDefinition {
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(crate) mode: RoleMode,
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_channel_settings_set"))]
pub(crate) struct ChannelSettingsSet {
    #[serde(flatten)]
    pub(crate) attributes: BTreeMap<String, toml::Value>,
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
];

fn validate_channel_definition(channel: &RawChannelDefinition) -> Result<(), ValidationError> {
    validate_channel_attributes(&channel.attributes)?;
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

use super::*;
