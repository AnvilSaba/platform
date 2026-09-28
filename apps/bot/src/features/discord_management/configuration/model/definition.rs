#[derive(Debug, Deserialize, Serialize, Validate)]
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
    #[validate(custom(function = "validate_message_set_names"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) message_sets: BTreeMap<String, RawMessageSetDefinition>,

    #[validate(nested)]
    #[validate(custom(function = "validate_thread_names"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) threads: BTreeMap<String, RawThreadDefinition>,

    #[validate(nested)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) order: Option<RawOrderDefinition>,
}

fn validate_resource_names<T>(resources: &BTreeMap<String, T>) -> Result<(), ValidationError> {
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

fn validate_message_set_names<T>(resources: &BTreeMap<String, T>) -> Result<(), ValidationError> {
    validate_resource_names(resources)
}

fn validate_thread_names<T>(resources: &BTreeMap<String, T>) -> Result<(), ValidationError> {
    validate_resource_names(resources)
}

#[derive(Debug)]
pub(crate) struct DefinitionFile {
    pub(crate) settings_sets: SettingsSets,
    pub(crate) roles: BTreeMap<RoleLogicalId, RoleDefinition>,
    pub(crate) channels: BTreeMap<ChannelLogicalId, ChannelDefinition>,
    pub(crate) members: BTreeMap<MemberLogicalId, MemberDefinition>,
    pub(crate) message_sets: BTreeMap<String, RawMessageSetDefinition>,
    pub(crate) threads: BTreeMap<String, RawThreadDefinition>,
    // order は parse/validate 済みの相対順序指定を保持します。
    pub(crate) order: Option<RawOrderDefinition>,
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
            order,
            schema_version: _,
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
        validate_order(order.as_ref(), &roles, &channels)?;
        Ok(Self {
            settings_sets,
            roles,
            channels,
            members,
            message_sets,
            threads,
            order,
        })
    }
}

/// 管理メッセージ群の入力を保持する typed model です。
///
/// message_set と thread は schema 上の許可される属性が異なるため、共通の
/// 自由形式モデルにせず、それぞれの入力境界で型付けと検証を行います。
#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_message_set_definition"))]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMessageSetDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ensure: Option<Ensure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) channel: Option<ChannelLogicalId>,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) body: Option<Vec<RawMessageDefinition>>,
}

/// 独立した管理スレッドの入力を保持する typed model です。
#[derive(Debug, Deserialize, Serialize, Validate)]
#[validate(schema(function = "validate_thread_definition"))]
#[serde(deny_unknown_fields)]
pub(crate) struct RawThreadDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ensure: Option<Ensure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) channel: Option<ChannelLogicalId>,
    #[validate(length(min = 1, max = 100, message = "name は1文字以上100文字以内で指定してください"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[validate(nested)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) body: Option<Vec<RawMessageDefinition>>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMessageDefinition {
    pub(crate) id: MessageLogicalId,
    #[validate(length(min = 1, message = "body は1文字以上で指定してください"))]
    pub(crate) body: String,
}

fn validate_message_set_definition(value: &RawMessageSetDefinition) -> Result<(), ValidationError> {
    if value.is_absent() {
        if value.channel.is_some() || value.body.is_some() {
            return Err(validation_error(
                "exclusive",
                "ensure = \"absent\" の管理メッセージ群には channel、body を指定できません",
            ));
        }
        return Ok(());
    }

    if value.channel.is_none() {
        return Err(validation_error("required", "管理メッセージ群には channel が必要です"));
    }
    let Some(body) = value.body.as_deref() else {
        return Err(validation_error("required", "管理メッセージ群には body が必要です"));
    };
    validate_message_ids(body)
}

fn validate_thread_definition(value: &RawThreadDefinition) -> Result<(), ValidationError> {
    if value.is_absent() {
        if value.channel.is_some() || value.name.is_some() || value.body.is_some() {
            return Err(validation_error(
                "exclusive",
                "ensure = \"absent\" の管理スレッドには channel、name、body を指定できません",
            ));
        }
        return Ok(());
    }

    if matches!(value.ensure, Some(Ensure::Present)) {
        return Err(validation_error(
            "exclusive",
            "管理スレッドの present には ensure を指定できません",
        ));
    }
    if value.channel.is_none() {
        return Err(validation_error("required", "管理スレッドには channel が必要です"));
    }
    if value.name.is_none() {
        return Err(validation_error("required", "管理スレッドには name が必要です"));
    }
    let Some(body) = value.body.as_deref() else {
        return Err(validation_error("required", "管理スレッドには body が必要です"));
    };
    validate_message_ids(body)
}

fn validate_message_ids(messages: &[RawMessageDefinition]) -> Result<(), ValidationError> {
    let mut ids = BTreeSet::new();
    for message in messages {
        if !ids.insert(&message.id) {
            return Err(validation_error(
                "duplicate",
                format!("body の message id {} が重複しています", message.id),
            ));
        }
    }
    Ok(())
}

pub(crate) trait RawResourceReference {
    fn is_absent(&self) -> bool;

    fn channel(&self) -> Option<&ChannelLogicalId>;
}

impl RawResourceReference for RawMessageSetDefinition {
    fn is_absent(&self) -> bool {
        matches!(self.ensure, Some(Ensure::Absent))
    }

    fn channel(&self) -> Option<&ChannelLogicalId> {
        self.channel.as_ref()
    }
}

impl RawResourceReference for RawThreadDefinition {
    fn is_absent(&self) -> bool {
        matches!(self.ensure, Some(Ensure::Absent))
    }

    fn channel(&self) -> Option<&ChannelLogicalId> {
        self.channel.as_ref()
    }
}

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOrderDefinition {
    #[validate(custom(function = "validate_unique_role_order"))]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) roles: Vec<RoleLogicalId>,
    #[validate(custom(function = "validate_unique_channel_order"))]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) categories: Vec<ChannelLogicalId>,
    #[validate(custom(function = "validate_unique_channel_order"))]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) uncategorized: Vec<ChannelLogicalId>,
    #[validate(custom(function = "validate_unique_children_order"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) children: BTreeMap<ChannelLogicalId, Vec<ChannelLogicalId>>,
}

fn validate_unique_role_order(values: &[RoleLogicalId]) -> Result<(), ValidationError> {
    validate_unique_order(values, "Role")?;
    if values.contains(&everyone_logical_id()) {
        return Err(validation_error(
            "fixed_position",
            "@everyone Role は最下位固定のため order.roles に指定できません",
        ));
    }
    Ok(())
}

fn validate_unique_channel_order(values: &[ChannelLogicalId]) -> Result<(), ValidationError> {
    validate_unique_order(values, "Channel")
}

fn validate_unique_children_order(
    values: &BTreeMap<ChannelLogicalId, Vec<ChannelLogicalId>>,
) -> Result<(), ValidationError> {
    for (parent, children) in values {
        validate_unique_order(children, &format!("Channel {parent} の子"))?;
    }
    Ok(())
}

fn validate_unique_order<T>(values: &[T], resource_kind: &str) -> Result<(), ValidationError>
where
    T: Ord + fmt::Display,
{
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(validation_error(
                "duplicate",
                format!("order の {resource_kind} {value} が重複しています"),
            ));
        }
    }
    Ok(())
}

fn validate_order(
    order: Option<&RawOrderDefinition>,
    roles: &BTreeMap<RoleLogicalId, RoleDefinition>,
    channels: &BTreeMap<ChannelLogicalId, ChannelDefinition>,
) -> Result<(), ManagementError> {
    let Some(order) = order else {
        return Ok(());
    };

    for logical_id in &order.roles {
        let Some(role) = roles.get(logical_id) else {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.roles の Role {logical_id} が宣言されていません"
            )));
        };
        if role.is_absent() {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.roles の Role {logical_id} は削除宣言のため順序指定できません"
            )));
        }
    }

    for logical_id in &order.categories {
        let Some(channel) = channels.get(logical_id) else {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.categories の Channel {logical_id} が宣言されていません"
            )));
        };
        validate_order_category(logical_id, channel)?;
    }

    let mut ordered_children = BTreeMap::new();
    for logical_id in &order.uncategorized {
        let Some(channel) = channels.get(logical_id) else {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.uncategorized の Channel {logical_id} が宣言されていません"
            )));
        };
        validate_order_uncategorized(logical_id, channel)?;
        ordered_children.insert(logical_id, None);
    }

    for (parent_id, children) in &order.children {
        let Some(parent) = channels.get(parent_id) else {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.children の親 Channel {parent_id} が宣言されていません"
            )));
        };
        validate_order_category(parent_id, parent)?;

        for child_id in children {
            if child_id == parent_id {
                return Err(ManagementError::InvalidDefinition(format!(
                    "order.children の Channel {child_id} は自身を親にできません"
                )));
            }
            if let Some(previous_parent) = ordered_children.insert(child_id, Some(parent_id)) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "order の Channel {child_id} が親 {previous_parent:?} と {parent_id} に重複指定されています"
                )));
            }
            let Some(child) = channels.get(child_id) else {
                return Err(ManagementError::InvalidDefinition(format!(
                    "order.children の子 Channel {child_id} が宣言されていません"
                )));
            };
            validate_order_child(parent_id, child_id, child)?;
        }
    }

    Ok(())
}

fn validate_order_uncategorized(
    logical_id: &ChannelLogicalId,
    channel: &ChannelDefinition,
) -> Result<(), ManagementError> {
    if channel.is_absent() {
        return Err(ManagementError::InvalidDefinition(format!(
            "order.uncategorized の Channel {logical_id} は削除宣言のため指定できません"
        )));
    }
    if channel.is_reference() {
        return Ok(());
    }
    let attributes = channel.attributes();
    if !matches!(attributes.kind, Some(ChannelKind::Text | ChannelKind::Announcement)) {
        return Err(ManagementError::InvalidDefinition(format!(
            "order.uncategorized の Channel {logical_id} には type = \"text\" または \"announcement\" が必要です"
        )));
    }
    if !matches!(attributes.parent, Some(ChannelValue::Clear)) {
        return Err(ManagementError::InvalidDefinition(format!(
            "order.uncategorized の Channel {logical_id} には parent = {{ clear = true }} が必要です"
        )));
    }
    Ok(())
}

fn validate_order_category(logical_id: &ChannelLogicalId, channel: &ChannelDefinition) -> Result<(), ManagementError> {
    if channel.is_absent() {
        return Err(ManagementError::InvalidDefinition(format!(
            "order の Category {logical_id} は削除宣言のため順序指定できません"
        )));
    }
    if channel.is_reference() {
        return Ok(());
    }
    match channel.attributes().kind {
        Some(ChannelKind::Category) => Ok(()),
        Some(ChannelKind::Text) => Err(ManagementError::InvalidDefinition(format!(
            "order の Category {logical_id} に Text Channel を指定できません"
        ))),
        Some(ChannelKind::Announcement) => Err(ManagementError::InvalidDefinition(format!(
            "order の Category {logical_id} に Announcement Channel を指定できません"
        ))),
        None => Err(ManagementError::InvalidDefinition(format!(
            "order の Category {logical_id} には type = \"category\" が必要です"
        ))),
        Some(ChannelKind::Unsupported) => unreachable!("定義ファイルから Unsupported Channel は生成されません"),
    }
}

fn validate_order_child(
    parent_id: &ChannelLogicalId,
    child_id: &ChannelLogicalId,
    child: &ChannelDefinition,
) -> Result<(), ManagementError> {
    if child.is_absent() {
        return Err(ManagementError::InvalidDefinition(format!(
            "order.children の子 Channel {child_id} は削除宣言のため指定できません"
        )));
    }
    if child.is_reference() {
        return Ok(());
    }

    let attributes = child.attributes();
    match attributes.kind {
        Some(ChannelKind::Category) => {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.children の子 Channel {child_id} は Category のため指定できません"
            )));
        }
        Some(ChannelKind::Text | ChannelKind::Announcement) => {}
        None => {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.children の子 Channel {child_id} には type が必要です"
            )));
        }
        Some(ChannelKind::Unsupported) => unreachable!("定義ファイルから Unsupported Channel は生成されません"),
    }

    let actual_parent = attributes.parent.as_ref().and_then(ChannelValue::as_value);
    if actual_parent != Some(parent_id) {
        return Err(ManagementError::InvalidDefinition(format!(
            "order.children の子 Channel {child_id} の親が {parent_id} と一致しません"
        )));
    }
    Ok(())
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
