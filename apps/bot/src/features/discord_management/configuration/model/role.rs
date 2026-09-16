use validator::ValidationError;

pub(crate) fn validate_definition(definition: &RawDefinitionFile) -> Result<(), ValidationError> {
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

#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRoleDefinition {
    #[serde(default, skip_serializing_if = "RoleEnsure::is_present")]
    pub(crate) ensure: RoleEnsure,
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(crate) mode: RoleMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) settings_sets: Vec<RoleSettingsSetId>,
    #[validate(nested)]
    #[serde(flatten)]
    pub(crate) attributes: RawRoleAttributes,
}

#[derive(Debug)]
pub(crate) enum RoleDefinition {
    Managed {
        settings_sets: Vec<RoleSettingsSetId>,
        attributes: RoleAttributes,
    },
    Reference,
    Absent,
}

impl RoleDefinition {
    pub(super) fn parse(
        logical_id: RoleLogicalId,
        raw: RawRoleDefinition,
        settings_sets: &BTreeMap<RoleSettingsSetId, RoleAttributes>,
        vocabulary: &PermissionVocabulary,
    ) -> Result<Self, ManagementError> {
        let attributes = raw.attributes.resolve(vocabulary, &format!("Role {logical_id}"))?;
        let mut seen = BTreeSet::new();
        for settings_set in &raw.settings_sets {
            if !seen.insert(settings_set) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Role {logical_id} で設定セット {settings_set} が重複しています"
                )));
            }
            if !settings_sets.contains_key(settings_set) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Role {logical_id} が未知の設定セット {settings_set} を参照しています"
                )));
            }
        }
        match (raw.ensure, raw.mode) {
            (RoleEnsure::Absent, RoleMode::Managed) if raw.settings_sets.is_empty() && attributes.is_empty() => {
                Ok(Self::Absent)
            }
            (RoleEnsure::Absent, _) => Err(ManagementError::InvalidDefinition(format!(
                "削除する Role {logical_id} には mode、設定セット、管理属性を指定できません"
            ))),
            (RoleEnsure::Present, RoleMode::Reference) if raw.settings_sets.is_empty() && attributes.is_empty() => {
                Ok(Self::Reference)
            }
            (RoleEnsure::Present, RoleMode::Reference) => Err(ManagementError::InvalidDefinition(format!(
                "参照専用 Role {logical_id} には管理属性を指定できません"
            ))),
            (RoleEnsure::Present, RoleMode::Managed) => Ok(Self::Managed {
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

    pub(crate) fn settings_sets(&self) -> &[RoleSettingsSetId] {
        match self {
            Self::Managed { settings_sets, .. } => settings_sets,
            Self::Reference | Self::Absent => &[],
        }
    }

    pub(crate) fn attributes(&self) -> &RoleAttributes {
        static EMPTY: std::sync::OnceLock<RoleAttributes> = std::sync::OnceLock::new();
        match self {
            Self::Managed { attributes, .. } => attributes,
            Self::Reference | Self::Absent => EMPTY.get_or_init(RoleAttributes::default),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoleEnsure {
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
pub(crate) enum RoleMode {
    #[default]
    Managed,
    Reference,
}

impl RoleMode {
    pub(super) fn is_managed(&self) -> bool {
        matches!(self, Self::Managed)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRoleAttributes {
    #[validate(custom(function = "validate_role_name"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<ManagedValue<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) color: Option<ManagedValue<Color>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) hoist: Option<ManagedValue<bool>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mentionable: Option<ManagedValue<bool>>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) permissions: BTreeMap<PermissionName, ManagedValue<bool>>,
}

fn validate_role_name(value: &ManagedValue<String>) -> Result<(), ValidationError> {
    let ManagedValue::Value(value) = value else {
        return Ok(());
    };
    if value.is_empty() {
        return Err(validation_error("length", "name は1文字以上で指定してください"));
    }
    if value.chars().count() > 100 {
        return Err(validation_error("length", "name は100文字以内で指定してください"));
    }
    Ok(())
}

impl RawRoleAttributes {
    pub(super) fn resolve(
        self,
        vocabulary: &PermissionVocabulary,
        context: &str,
    ) -> Result<RoleAttributes, ManagementError> {
        let Self {
            name,
            color,
            hoist,
            mentionable,
            permissions,
        } = self;
        let permissions = permissions
            .into_iter()
            .map(|(permission, value)| {
                let known = vocabulary.resolve(&permission).ok_or_else(|| {
                    ManagementError::InvalidDefinition(format!(
                        "{context} に未知の権限 {permission} が指定されています"
                    ))
                })?;
                Ok((known, value))
            })
            .collect::<Result<_, ManagementError>>()?;
        Ok(RoleAttributes {
            name,
            color,
            hoist,
            mentionable,
            permissions,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RoleAttributes {
    pub(crate) name: Option<ManagedValue<String>>,
    pub(crate) color: Option<ManagedValue<Color>>,
    pub(crate) hoist: Option<ManagedValue<bool>>,
    pub(crate) mentionable: Option<ManagedValue<bool>>,
    pub(crate) permissions: BTreeMap<KnownPermission, ManagedValue<bool>>,
}

impl RoleAttributes {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.color.is_none()
            && self.hoist.is_none()
            && self.mentionable.is_none()
            && self.permissions.is_empty()
    }

    pub(crate) fn has_non_permission_attributes(&self) -> bool {
        self.name.is_some() || self.color.is_some() || self.hoist.is_some() || self.mentionable.is_some()
    }

    pub(crate) fn merge(&mut self, later: &Self) {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ManagedValue<T> {
    Value(T),
    Default,
}

impl<T> ManagedValue<T> {
    pub(crate) fn as_value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Default => None,
        }
    }

    pub(crate) fn into_value(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Default => None,
        }
    }

    pub(crate) fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }

    pub(crate) fn resolve(&self, default: T) -> T
    where
        T: Clone,
    {
        if self.is_default() {
            default
        } else {
            self.clone()
                .into_value()
                .expect("Default 以外の ManagedValue は Value です")
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawManagedValue<T> {
    Value(T),
    Default { default: bool },
}

impl<'de, T> serde::Deserialize<'de> for ManagedValue<T>
where
    T: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match RawManagedValue::deserialize(deserializer)? {
            RawManagedValue::Value(value) => Ok(Self::Value(value)),
            RawManagedValue::Default { default: true } => Ok(Self::Default),
            RawManagedValue::Default { default: false } => {
                Err(de::Error::custom("default 指定は true である必要があります"))
            }
        }
    }
}

impl<T> serde::Serialize for ManagedValue<T>
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
        }
    }
}

pub(crate) fn everyone_logical_id() -> RoleLogicalId {
    RoleLogicalId::parse("everyone").expect("予約済み論理 ID は常に有効です")
}

pub(crate) fn resolve_role_id(logical_id: &RoleLogicalId, state: &StateFile) -> Result<RoleId, ManagementError> {
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

use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Color(u32);

impl Color {
    pub fn new(value: u32) -> Result<Self, &'static str> {
        (value <= 0xFF_FF_FF)
            .then_some(Self(value))
            .ok_or("color は 0 から 16777215 の範囲で指定してください")
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}
