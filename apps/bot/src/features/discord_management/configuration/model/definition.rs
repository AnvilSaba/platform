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
    #[serde(default, skip_serializing_if = "RawSettingsSets::is_empty")]
    pub(crate) settings_sets: RawSettingsSets,

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
    pub(crate) message_sets: BTreeMap<String, RawResourceDefinition>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) threads: BTreeMap<String, RawResourceDefinition>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) order: Option<RawResourceDefinition>,
}

#[derive(Debug)]
pub(crate) struct DefinitionFile {
    pub(crate) settings_sets: SettingsSets,
    pub(crate) roles: BTreeMap<RoleLogicalId, RoleDefinition>,
    pub(crate) channels: BTreeMap<ChannelLogicalId, ChannelDefinition>,
    pub(crate) members: BTreeMap<MemberLogicalId, MemberDefinition>,
    pub(crate) message_sets: BTreeMap<String, RawResourceDefinition>,
    pub(crate) threads: BTreeMap<String, RawResourceDefinition>,
}

impl DefinitionFile {
    pub(crate) fn parse(contents: &str, vocabulary: &PermissionVocabulary) -> Result<Self, ManagementError> {
        let raw: RawDefinitionFile =
            toml::from_str(contents).map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        raw.validate()
            .map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;

        let RawDefinitionFile {
            settings_sets: raw_settings_sets,
            roles: raw_roles,
            channels: raw_channels,
            members,
            message_sets,
            threads,
            ..
        } = raw;
        let settings_sets = raw_settings_sets.resolve(vocabulary)?;

        let roles = raw_roles
            .into_iter()
            .map(|(logical_id, role)| {
                RoleDefinition::parse(logical_id.clone(), role, &settings_sets.role, vocabulary)
                    .map(|role| (logical_id, role))
            })
            .collect::<Result<_, _>>()?;
        let channels = raw_channels
            .into_iter()
            .map(|(logical_id, channel)| {
                ChannelDefinition::parse(logical_id.clone(), channel, &settings_sets.channel, vocabulary)
                    .map(|channel| (logical_id, channel))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            settings_sets,
            roles,
            channels,
            members,
            message_sets,
            threads,
        })
    }
}

/// 未実装の message set / thread 定義を TOML adapter 内に保持します。
///
/// Channel の設定モデルへ自由形式の `toml::Value` を渡さないための隔離 seam です。
#[derive(Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct RawResourceDefinition(toml::Value);

impl RawResourceDefinition {
    pub(crate) fn is_table(&self) -> bool {
        self.0.is_table()
    }

    pub(crate) fn is_absent(&self) -> bool {
        self.table()
            .and_then(|table| table.get("ensure"))
            .and_then(toml::Value::as_str)
            .is_some_and(|ensure| ensure == "absent")
    }

    pub(crate) fn has_channel(&self) -> bool {
        self.table().is_some_and(|table| table.contains_key("channel"))
    }

    pub(crate) fn channel_name(&self) -> Option<&str> {
        self.table()
            .and_then(|table| table.get("channel"))
            .and_then(toml::Value::as_str)
    }

    fn table(&self) -> Option<&toml::map::Map<String, toml::Value>> {
        self.0.as_table()
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSettingsSets {
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) role: BTreeMap<RoleSettingsSetId, RawRoleAttributes>,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) channel: BTreeMap<ChannelSettingsSetId, ChannelSettingsSet>,
}

impl RawSettingsSets {
    fn is_empty(&self) -> bool {
        self.role.is_empty() && self.channel.is_empty()
    }

    fn resolve(self, vocabulary: &PermissionVocabulary) -> Result<SettingsSets, ManagementError> {
        let role = self
            .role
            .into_iter()
            .map(|(name, attributes)| {
                attributes
                    .resolve(vocabulary, &format!("Role 設定セット {name}"))
                    .map(|attributes| (name, attributes))
            })
            .collect::<Result<_, _>>()?;
        let channel = self
            .channel
            .into_iter()
            .map(|(name, settings)| {
                let logical_id = ChannelLogicalId::parse(format!("settings_set_{name}"))
                    .expect("設定セット検証用の論理 ID は常に有効です");
                settings
                    .attributes
                    .resolve(&logical_id, vocabulary)
                    .map(|attributes| (name, attributes))
            })
            .collect::<Result<_, _>>()?;
        Ok(SettingsSets { role, channel })
    }
}

#[derive(Debug, Default)]
pub(crate) struct SettingsSets {
    pub(crate) role: BTreeMap<RoleSettingsSetId, RoleAttributes>,
    pub(crate) channel: BTreeMap<ChannelSettingsSetId, ChannelAttributes>,
}

use super::*;
