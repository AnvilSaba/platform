use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeMap};

use super::{KnownPermission, PermissionName, PermissionVocabulary};
use crate::features::discord_management::{domain::ManagementError, ids::ChannelLogicalId};

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChannelValue<T> {
    Value(T),
    Default,
    Clear,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawChannelValue<T> {
    Value(T),
    Marker(RawChannelMarker),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChannelMarker {
    #[serde(default)]
    default: Option<bool>,
    #[serde(default)]
    clear: Option<bool>,
}

impl<'de, T> serde::Deserialize<'de> for ChannelValue<T>
where
    T: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match RawChannelValue::deserialize(deserializer)? {
            RawChannelValue::Value(value) => Ok(Self::Value(value)),
            RawChannelValue::Marker(RawChannelMarker { default, clear }) => match (default, clear) {
                (Some(true), None) => Ok(Self::Default),
                (None, Some(true)) => Ok(Self::Clear),
                (Some(false), None) => Err(de::Error::custom("default 指定は true である必要があります")),
                (None, Some(false)) => Err(de::Error::custom("clear 指定は true である必要があります")),
                _ => Err(de::Error::custom(
                    "default = true または clear = true のいずれか一つを指定してください",
                )),
            },
        }
    }
}

impl<T> serde::Serialize for ChannelValue<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Default => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("default", &true)?;
                map.end()
            }
            Self::Clear => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("clear", &true)?;
                map.end()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
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
                || self.nsfw.is_some()
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawChannelAttributes {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<ChannelValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) parent: Option<ChannelValue<ChannelLogicalId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) topic: Option<ChannelValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) nsfw: Option<ChannelValue<bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) slowmode_seconds: Option<ChannelValue<u16>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) default_auto_archive_minutes: Option<ChannelValue<u16>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) default_thread_slowmode_seconds: Option<ChannelValue<u16>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) overwrites: BTreeMap<String, BTreeMap<PermissionName, OverwriteValue>>,
}

impl RawChannelAttributes {
    pub(crate) fn resolve(
        self,
        logical_id: &ChannelLogicalId,
        vocabulary: &PermissionVocabulary,
    ) -> Result<ChannelAttributes, ManagementError> {
        let Self {
            kind,
            name,
            parent,
            topic,
            nsfw,
            slowmode_seconds,
            default_auto_archive_minutes,
            default_thread_slowmode_seconds,
            overwrites,
        } = self;
        let kind = kind
            .map(|kind| match kind.as_str() {
                "category" => Ok(ChannelKind::Category),
                "text" => Ok(ChannelKind::Text),
                _ => Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} の type は category または text で指定してください"
                ))),
            })
            .transpose()?;
        let name = name
            .map(|value| validate_string_value(value, logical_id, "name", false, 100))
            .transpose()?;
        let topic = topic
            .map(|value| validate_string_value(value, logical_id, "topic", true, 1024))
            .transpose()?;
        let nsfw = nsfw
            .map(|value| validate_bool_value(value, logical_id, "nsfw"))
            .transpose()?;
        let slowmode_seconds = slowmode_seconds
            .map(|value| validate_u16_value(value, logical_id, "slowmode_seconds", true))
            .transpose()?;
        let default_auto_archive_minutes = default_auto_archive_minutes
            .map(|value| validate_auto_archive_value(value, logical_id))
            .transpose()?;
        let default_thread_slowmode_seconds = default_thread_slowmode_seconds
            .map(|value| validate_u16_value(value, logical_id, "default_thread_slowmode_seconds", true))
            .transpose()?;
        let overwrites = resolve_overwrites(overwrites, logical_id, vocabulary)?;
        Ok(ChannelAttributes {
            kind,
            name,
            parent,
            topic,
            nsfw,
            slowmode_seconds,
            default_auto_archive_minutes,
            default_thread_slowmode_seconds,
            overwrites,
        })
    }
}

fn validate_string_value(
    value: ChannelValue<String>,
    logical_id: &ChannelLogicalId,
    attribute: &str,
    allow_clear: bool,
    max_length: usize,
) -> Result<ChannelValue<String>, ManagementError> {
    match &value {
        ChannelValue::Value(value) => {
            if attribute == "name" && value.is_empty() {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} の name は1文字以上で指定してください"
                )));
            }
            if value.chars().count() > max_length {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} の {attribute} は{max_length}文字以内で指定してください"
                )));
            }
        }
        ChannelValue::Clear if !allow_clear => {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の {attribute} は解除できません"
            )));
        }
        ChannelValue::Default | ChannelValue::Clear => {}
    }
    Ok(value)
}

fn validate_bool_value(
    value: ChannelValue<bool>,
    logical_id: &ChannelLogicalId,
    attribute: &str,
) -> Result<ChannelValue<bool>, ManagementError> {
    if matches!(value, ChannelValue::Clear) {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は解除できません"
        )));
    }
    Ok(value)
}

fn validate_u16_value(
    value: ChannelValue<u16>,
    logical_id: &ChannelLogicalId,
    attribute: &str,
    allow_clear: bool,
) -> Result<ChannelValue<u16>, ManagementError> {
    match value {
        ChannelValue::Value(value) => {
            if value > 21_600 && matches!(attribute, "slowmode_seconds" | "default_thread_slowmode_seconds") {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} の {attribute} は0から21600の範囲で指定してください"
                )));
            }
            Ok(ChannelValue::Value(value))
        }
        ChannelValue::Clear if allow_clear => Ok(ChannelValue::Clear),
        ChannelValue::Clear => Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の {attribute} は解除できません"
        ))),
        ChannelValue::Default => Ok(ChannelValue::Default),
    }
}

fn validate_auto_archive_value(
    value: ChannelValue<u16>,
    logical_id: &ChannelLogicalId,
) -> Result<ChannelValue<u16>, ManagementError> {
    match value {
        ChannelValue::Value(value) if matches!(value, 60 | 1440 | 4320 | 10080) => Ok(ChannelValue::Value(value)),
        ChannelValue::Value(_) => Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の default_auto_archive_minutes は60、1440、4320、10080のいずれかで指定してください"
        ))),
        ChannelValue::Default => Ok(ChannelValue::Default),
        ChannelValue::Clear => Ok(ChannelValue::Clear),
    }
}

fn resolve_overwrites(
    overwrites: BTreeMap<String, BTreeMap<PermissionName, OverwriteValue>>,
    logical_id: &ChannelLogicalId,
    vocabulary: &PermissionVocabulary,
) -> Result<BTreeMap<String, BTreeMap<KnownPermission, OverwriteValue>>, ManagementError> {
    overwrites
        .into_iter()
        .map(|(subject, permissions)| {
            if subject != "everyone" && !(subject.starts_with("role:") || subject.starts_with("member:")) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {logical_id} の権限対象 {subject} は everyone、role:<論理 ID>、member:<論理 ID> のいずれかで指定してください"
                )));
            }
            if let Some(role) = subject.strip_prefix("role:") {
                crate::features::discord_management::ids::RoleLogicalId::parse(role).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Channel {logical_id} の {subject} が不正です: {error}"))
                })?;
            }
            if let Some(member) = subject.strip_prefix("member:") {
                crate::features::discord_management::ids::MemberLogicalId::parse(member).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Channel {logical_id} の {subject} が不正です: {error}"))
                })?;
            }
            let permissions = permissions
                .into_iter()
                .map(|(permission, value)| {
                    let name = permission.as_str();
                    let known = vocabulary.resolve(&permission).ok_or_else(|| {
                        ManagementError::InvalidDefinition(format!(
                            "Channel {logical_id} の {subject} に未知の権限 {name} が指定されています"
                        ))
                    })?;
                    Ok((known, value))
                })
                .collect::<Result<_, ManagementError>>()?;
            Ok((subject, permissions))
        })
        .collect()
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub(crate) struct RawChannelDefinition {
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(crate) mode: RoleMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ensure: Option<Ensure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) settings_sets: Vec<ChannelSettingsSetId>,
    #[validate(nested)]
    #[serde(flatten)]
    pub(crate) attributes: RawChannelAttributes,
}

#[derive(Debug)]
pub(crate) enum ChannelDefinition {
    Managed { attributes: ChannelAttributes },
    Reference,
    Absent,
}

impl ChannelDefinition {
    pub(super) fn parse(
        logical_id: ChannelLogicalId,
        raw: RawChannelDefinition,
        settings_sets: &BTreeMap<ChannelSettingsSetId, ChannelAttributes>,
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
            attributes.merge(settings);
        }
        let direct_attributes = raw.attributes.resolve(&logical_id, vocabulary)?;
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
            (None | Some(Ensure::Present), RoleMode::Managed) => Ok(Self::Managed { attributes }),
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
pub(crate) struct ChannelSettingsSet {
    #[validate(nested)]
    #[serde(flatten)]
    pub(crate) attributes: RawChannelAttributes,
}

use super::*;
