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
        // TODO(#7, #10-#13): Parse channel attributes into channel-type-specific
        // domain models and validate the composed settings.
        attributes: BTreeMap<String, toml::Value>,
    },
    Reference,
    Absent,
}

impl ChannelDefinition {
    pub(super) fn parse(
        logical_id: ChannelLogicalId,
        raw: RawChannelDefinition,
        settings_sets: &BTreeMap<ChannelSettingsSetId, ChannelSettingsSet>,
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

        match (raw.ensure, raw.mode) {
            (Some(Ensure::Absent), RoleMode::Managed) if raw.settings_sets.is_empty() && raw.attributes.is_empty() => {
                Ok(Self::Absent)
            }
            (Some(Ensure::Absent), _) => Err(ManagementError::InvalidDefinition(format!(
                "削除宣言 Channel {logical_id} には mode や管理属性を指定できません"
            ))),
            (None, RoleMode::Reference) if raw.settings_sets.is_empty() && raw.attributes.is_empty() => {
                Ok(Self::Reference)
            }
            (_, RoleMode::Reference) => Err(ManagementError::InvalidDefinition(format!(
                "参照専用 Channel {logical_id} には ensure や管理属性を指定できません"
            ))),
            (None | Some(Ensure::Present), RoleMode::Managed) => Ok(Self::Managed {
                settings_sets: raw.settings_sets,
                attributes: raw.attributes,
            }),
        }
    }

    pub(crate) fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }

    pub(crate) fn attributes(&self) -> &BTreeMap<String, toml::Value> {
        static EMPTY: std::sync::OnceLock<BTreeMap<String, toml::Value>> = std::sync::OnceLock::new();
        match self {
            Self::Managed { attributes, .. } => attributes,
            Self::Reference | Self::Absent => EMPTY.get_or_init(BTreeMap::new),
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
