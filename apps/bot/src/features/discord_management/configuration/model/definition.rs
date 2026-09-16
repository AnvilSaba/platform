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

    #[validate(nested)]
    #[validate(custom(function = "validate_resource_names"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) message_sets: BTreeMap<String, RawResourceDefinition>,

    #[validate(nested)]
    #[validate(custom(function = "validate_resource_names"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) threads: BTreeMap<String, RawResourceDefinition>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) order: Option<RawOrderDefinition>,
}

fn validate_definition(definition: &RawDefinitionFile) -> Result<(), ValidationError> {
    for (name, thread) in &definition.threads {
        if !thread.is_absent() && thread.name.is_none() {
            return Err(validation_error(
                "required",
                format!("管理スレッド {name} には name が必要です"),
            ));
        }
    }
    Ok(())
}

fn validate_resource_names(resources: &BTreeMap<String, RawResourceDefinition>) -> Result<(), ValidationError> {
    for name in resources.keys() {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(validation_error(
                "format",
                format!("管理リソース名 {name} は英数字、ハイフン、アンダースコアだけで指定してください"),
            ));
        }
    }
    Ok(())
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

/// 管理メッセージ群・管理スレッドの入力を保持する typed model です。
///
/// これらの実装はまだ別Featureですが、設定モデルから自由形式の
/// `toml::Value` を引き回さないため、現在のスキーマに対応する範囲だけを型付けします。
#[derive(Debug, Serialize, Validate)]
#[validate(schema(function = "validate_resource_definition"))]
#[serde(deny_unknown_fields)]
pub(crate) struct RawResourceDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ensure: Option<Ensure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) channel: Option<ChannelLogicalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[validate(length(min = 1, max = 100, message = "name は1文字以上100文字以内で指定してください"))]
    pub(crate) name: Option<String>,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) body: Vec<RawMessageDefinition>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMessageDefinition {
    pub(crate) id: MessageLogicalId,
    #[validate(length(min = 1, message = "body は1文字以上で指定してください"))]
    pub(crate) body: String,
}

fn validate_resource_definition(value: &RawResourceDefinition) -> Result<(), ValidationError> {
    if value.is_absent() {
        if value.channel.is_some() || value.name.is_some() || !value.body.is_empty() {
            return Err(validation_error(
                "exclusive",
                "ensure = \"absent\" の管理リソースには channel、name、body を指定できません",
            ));
        }
        return Ok(());
    }

    if value.channel.is_none() {
        return Err(validation_error("required", "管理リソースには channel が必要です"));
    }

    let mut ids = BTreeSet::new();
    for message in &value.body {
        if !ids.insert(&message.id) {
            return Err(validation_error(
                "duplicate",
                format!("body の message id {} が重複しています", message.id),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOrderDefinition {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) categories: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) children: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResourceDefinitionWire {
    #[serde(default)]
    ensure: Option<Ensure>,
    #[serde(default)]
    channel: Option<ChannelLogicalId>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Vec<RawMessageDefinition>,
}

impl From<RawResourceDefinitionWire> for RawResourceDefinition {
    fn from(value: RawResourceDefinitionWire) -> Self {
        Self {
            ensure: value.ensure,
            channel: value.channel,
            name: value.name,
            body: value.body,
        }
    }
}

impl<'de> Deserialize<'de> for RawResourceDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = toml::Value::deserialize(deserializer)?;
        if !value.is_table() {
            return Err(de::Error::custom("管理リソース定義はテーブルで指定してください"));
        }
        let wire = value
            .try_into::<RawResourceDefinitionWire>()
            .map_err(de::Error::custom)?;
        Ok(wire.into())
    }
}

impl RawResourceDefinition {
    pub(crate) fn is_absent(&self) -> bool {
        matches!(self.ensure, Some(Ensure::Absent))
    }

    pub(crate) fn channel(&self) -> Option<&ChannelLogicalId> {
        self.channel.as_ref()
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
