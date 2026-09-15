#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_definition"))]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDefinitionFile {
    #[validate(range(
        min = "SCHEMA_VERSION",
        max = "SCHEMA_VERSION",
        message = "対応していない schema_version です"
    ))]
    pub(crate) schema_version: u32,

    #[validate(nested)]
    #[serde(default, skip_serializing_if = "SettingsSets::is_empty")]
    pub(crate) settings_sets: SettingsSets,

    #[validate(nested)]
    #[serde(default)]
    pub(crate) roles: BTreeMap<RoleLogicalId, RawRoleDefinition>,

    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) channels: BTreeMap<ChannelLogicalId, RawChannelDefinition>,

    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) members: BTreeMap<MemberLogicalId, MemberDefinition>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) message_sets: BTreeMap<String, toml::Value>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) threads: BTreeMap<String, toml::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) order: Option<toml::Value>,
}

#[derive(Debug)]
pub(crate) struct DefinitionFile {
    pub(crate) settings_sets: SettingsSets,
    pub(crate) roles: BTreeMap<RoleLogicalId, RoleDefinition>,
    pub(crate) channels: BTreeMap<ChannelLogicalId, ChannelDefinition>,
    pub(crate) members: BTreeMap<MemberLogicalId, MemberDefinition>,
    pub(crate) message_sets: BTreeMap<String, toml::Value>,
    pub(crate) threads: BTreeMap<String, toml::Value>,
}

impl DefinitionFile {
    pub(crate) fn parse(contents: &str) -> Result<Self, ManagementError> {
        let raw: RawDefinitionFile =
            toml::from_str(contents).map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        raw.validate()
            .map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;

        let roles = raw
            .roles
            .into_iter()
            .map(|(logical_id, role)| {
                RoleDefinition::parse(logical_id.clone(), role, &raw.settings_sets.role).map(|role| (logical_id, role))
            })
            .collect::<Result<_, _>>()?;
        let channels = raw
            .channels
            .into_iter()
            .map(|(logical_id, channel)| {
                ChannelDefinition::parse(logical_id.clone(), channel, &raw.settings_sets.channel)
                    .map(|channel| (logical_id, channel))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            settings_sets: raw.settings_sets,
            roles,
            channels,
            members: raw.members,
            message_sets: raw.message_sets,
            threads: raw.threads,
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsSets {
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) role: BTreeMap<RoleSettingsSetId, RoleAttributes>,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) channel: BTreeMap<ChannelSettingsSetId, ChannelSettingsSet>,
}

impl SettingsSets {
    fn is_empty(&self) -> bool {
        self.role.is_empty() && self.channel.is_empty()
    }
}

use super::*;
