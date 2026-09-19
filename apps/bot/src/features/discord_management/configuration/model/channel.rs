use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeMap};
use validator::{ValidateLength, ValidateRange, ValidationError};

use super::{KnownPermission, PermissionName, PermissionVocabulary};
use crate::features::discord_management::{
    domain::ManagementError,
    ids::{ChannelLogicalId, MemberLogicalId, RoleLogicalId},
};

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

/// 定義ファイルで指定できる Channel 種別です。
///
/// `ChannelKind::Unsupported` は Discord のカタログを読むときだけ必要な値なので、
/// 定義ファイルの raw model には含めません。serde に字句の解釈を任せることで、
/// resolve 側に種別文字列の手書き判定を残さないようにします。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RawChannelKind {
    Category,
    Text,
}

impl From<RawChannelKind> for ChannelKind {
    fn from(kind: RawChannelKind) -> Self {
        match kind {
            RawChannelKind::Category => Self::Category,
            RawChannelKind::Text => Self::Text,
        }
    }
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

impl<T> ChannelValue<T> {
    pub(crate) fn as_value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Default | Self::Clear => None,
        }
    }

    pub(crate) fn into_value(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Default | Self::Clear => None,
        }
    }

    pub(crate) fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }

    pub(crate) fn is_clear(&self) -> bool {
        matches!(self, Self::Clear)
    }

    /// `Default` と `Clear` を呼び出し側の値へ解決します。
    ///
    /// `clear` は属性ごとに意味が異なるため、解決済みの値を呼び出し側が渡します。
    /// 解除が `None` を意味する属性では `resolve_optional` を使用してください。
    pub(crate) fn resolve(&self, default: T, clear: T) -> T
    where
        T: Clone,
    {
        match self {
            Self::Value(value) => value.clone(),
            Self::Default => default,
            Self::Clear => clear,
        }
    }

    pub(crate) fn resolve_optional(&self, default: Option<T>) -> Option<T>
    where
        T: Clone,
    {
        if let Some(value) = self.clone().into_value() {
            return Some(value);
        }
        match self {
            Self::Default => default,
            Self::Clear => None,
            Self::Value(_) => unreachable!("Value は into_value で先に取り出されます"),
        }
    }
}

impl<T, U> ValidateLength<U> for ChannelValue<T>
where
    T: ValidateLength<U>,
    U: PartialEq + PartialOrd,
{
    fn length(&self) -> Option<U> {
        match self {
            Self::Value(value) => value.length(),
            Self::Default | Self::Clear => None,
        }
    }
}

impl<T, U> ValidateRange<U> for ChannelValue<T>
where
    T: ValidateRange<U>,
{
    fn greater_than(&self, max: U) -> Option<bool> {
        match self {
            Self::Value(value) => value.greater_than(max),
            Self::Default | Self::Clear => None,
        }
    }

    fn less_than(&self, min: U) -> Option<bool> {
        match self {
            Self::Value(value) => value.less_than(min),
            Self::Default | Self::Clear => None,
        }
    }
}

/// Channel の permission overwrite を設定ファイル上で識別する typed target です。
///
/// 論理 ID の字句検証は Deserialize 時に済ませ、state や definition の存在確認は
/// 外部文脈を持つ `PlanInput` 側で行います。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum OverwriteTarget {
    Everyone,
    Role(RoleLogicalId),
    Member(MemberLogicalId),
}

impl fmt::Display for OverwriteTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Everyone => formatter.write_str("everyone"),
            Self::Role(logical_id) => write!(formatter, "role:{logical_id}"),
            Self::Member(logical_id) => write!(formatter, "member:{logical_id}"),
        }
    }
}

impl<'de> Deserialize<'de> for OverwriteTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == "everyone" {
            return Ok(Self::Everyone);
        }
        if let Some(logical_id) = value.strip_prefix("role:") {
            if logical_id == "everyone" {
                return Err(de::Error::custom(
                    "権限対象 role:everyone は使用できません。everyone を指定してください",
                ));
            }
            return RoleLogicalId::parse(logical_id)
                .map(Self::Role)
                .map_err(de::Error::custom);
        }
        if let Some(logical_id) = value.strip_prefix("member:") {
            return MemberLogicalId::parse(logical_id)
                .map(Self::Member)
                .map_err(de::Error::custom);
        }
        Err(de::Error::custom(format!(
            "権限対象 {value} は everyone、role:<論理 ID>、member:<論理 ID> のいずれかで指定してください"
        )))
    }
}

impl Serialize for OverwriteTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
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
    /// 親 Category と permission overwrite を同期する明示指定です。
    pub(crate) permissions_sync: Option<bool>,
    pub(crate) overwrites: BTreeMap<OverwriteTarget, BTreeMap<KnownPermission, OverwriteValue>>,
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
        if later.permissions_sync.is_some() {
            self.permissions_sync = later.permissions_sync;
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
        if self.permissions_sync == Some(true) && !self.overwrites.is_empty() {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の permissions_sync と個別 Overwrite は併用できません"
            )));
        }
        if self.permissions_sync == Some(true) && kind == ChannelKind::Category {
            return Err(ManagementError::InvalidDefinition(format!(
                "Category {logical_id} には permissions_sync を指定できません"
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
            && self.permissions_sync.is_none()
            && self.overwrites.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawChannelAttributes {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<RawChannelKind>,
    #[validate(length(min = 1, max = 100, message = "name は1文字以上かつ100文字以内で指定してください"))]
    #[validate(custom(function = "validate_channel_name_markers"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<ChannelValue<String>>,
    #[validate(custom(function = "validate_channel_parent"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) parent: Option<ChannelValue<ChannelLogicalId>>,
    #[validate(length(max = 1024, message = "topic は1024文字以内で指定してください"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) topic: Option<ChannelValue<String>>,
    #[validate(custom(function = "validate_channel_nsfw"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) nsfw: Option<ChannelValue<bool>>,
    #[validate(range(max = 21600, message = "slowmode_seconds は0から21600の範囲で指定してください"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) slowmode_seconds: Option<ChannelValue<u16>>,
    #[validate(custom(function = "validate_channel_auto_archive"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) default_auto_archive_minutes: Option<ChannelValue<u16>>,
    #[validate(range(
        max = 21600,
        message = "default_thread_slowmode_seconds は0から21600の範囲で指定してください"
    ))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) default_thread_slowmode_seconds: Option<ChannelValue<u16>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) permissions_sync: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) overwrites: BTreeMap<OverwriteTarget, BTreeMap<PermissionName, OverwriteValue>>,
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
            permissions_sync,
            overwrites,
        } = self;
        let kind = kind.map(ChannelKind::from);
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
            permissions_sync,
            overwrites,
        })
    }
}

fn validation_error(code: &'static str, message: impl Into<String>) -> ValidationError {
    ValidationError::new(code).with_message(message.into().into())
}

fn validate_channel_parent(value: &ChannelValue<ChannelLogicalId>) -> Result<(), ValidationError> {
    validate_channel_markers(value, "parent", false, true)
}

fn validate_channel_name_markers(value: &ChannelValue<String>) -> Result<(), ValidationError> {
    validate_channel_markers(value, "name", false, false)
}

fn validate_channel_nsfw(value: &ChannelValue<bool>) -> Result<(), ValidationError> {
    validate_channel_markers(value, "nsfw", true, false)
}

fn validate_channel_markers<T>(
    value: &ChannelValue<T>,
    attribute: &'static str,
    allow_default: bool,
    allow_clear: bool,
) -> Result<(), ValidationError> {
    match value {
        ChannelValue::Default if !allow_default => Err(validation_error(
            "invalid_marker",
            format!("{attribute} に default は指定できません"),
        )),
        ChannelValue::Clear if !allow_clear => Err(validation_error(
            "invalid_marker",
            format!("{attribute} は解除できません"),
        )),
        _ => Ok(()),
    }
}

fn validate_channel_auto_archive(value: &ChannelValue<u16>) -> Result<(), ValidationError> {
    match value {
        ChannelValue::Value(value) if !matches!(value, 60 | 1440 | 4320 | 10080) => Err(validation_error(
            "allowed_values",
            "default_auto_archive_minutes は60、1440、4320、10080のいずれかで指定してください",
        )),
        _ => Ok(()),
    }
}

fn resolve_overwrites(
    overwrites: BTreeMap<OverwriteTarget, BTreeMap<PermissionName, OverwriteValue>>,
    logical_id: &ChannelLogicalId,
    vocabulary: &PermissionVocabulary,
) -> Result<BTreeMap<OverwriteTarget, BTreeMap<KnownPermission, OverwriteValue>>, ManagementError> {
    overwrites
        .into_iter()
        .map(|(subject, permissions)| {
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
        let mut declared_kind = None;
        for settings_set in &raw.settings_sets {
            let settings = settings_sets
                .get(settings_set)
                .expect("検証済み Channel 定義は既知の設定セットだけを参照します");
            if let Some(kind) = settings.kind {
                if let Some((declared, declared_by)) = &declared_kind
                    && *declared != kind
                {
                    return Err(ManagementError::InvalidDefinition(format!(
                        "Channel {logical_id} の設定セット {settings_set} の type {} は設定セット {declared_by} の type と一致しません",
                        kind.as_str()
                    )));
                }
                declared_kind = Some((kind, settings_set.clone()));
            }
            attributes.merge(settings);
        }
        let direct_attributes = raw.attributes.resolve(&logical_id, vocabulary)?;
        if let Some(kind) = direct_attributes.kind
            && let Some((declared, declared_by)) = &declared_kind
            && *declared != kind
        {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の直接指定 type {} は設定セット {declared_by} の type と一致しません",
                kind.as_str()
            )));
        }
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
    #[validate(custom(function = "validate_member_mode"))]
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(crate) mode: RoleMode,
}

fn validate_member_mode(value: &RoleMode) -> Result<(), ValidationError> {
    if matches!(value, RoleMode::Reference) {
        Ok(())
    } else {
        Err(ValidationError::new("member_must_be_reference")
            .with_message("Member は参照専用として宣言してください".into()))
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
pub(crate) struct ChannelSettingsSet {
    #[validate(nested)]
    #[serde(flatten)]
    pub(crate) attributes: RawChannelAttributes,
}

use super::*;
