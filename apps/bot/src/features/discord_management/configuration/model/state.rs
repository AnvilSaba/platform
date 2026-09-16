#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawStateFile {
    #[validate(range(
        min = "SCHEMA_VERSION",
        max = "SCHEMA_VERSION",
        message = "対応していない schema_version です"
    ))]
    pub(crate) schema_version: u32,

    pub(crate) guild_id: GuildId,

    #[validate(custom(function = "validate_role_mappings"))]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_unique_role_mappings")]
    pub(crate) roles: BTreeMap<RoleLogicalId, RoleId>,

    #[validate(custom(function = "validate_channel_mappings"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_unique_channel_mappings")]
    pub(crate) channels: BTreeMap<ChannelLogicalId, ChannelId>,

    #[validate(custom(function = "validate_member_mappings"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "deserialize_unique_member_mappings")]
    pub(crate) members: BTreeMap<MemberLogicalId, MemberId>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StateFile {
    pub(crate) schema_version: u32,

    pub(crate) guild_id: GuildId,

    pub(crate) roles: BTreeMap<RoleLogicalId, RoleId>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) channels: BTreeMap<ChannelLogicalId, ChannelId>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) members: BTreeMap<MemberLogicalId, MemberId>,
}

impl StateFile {
    pub(crate) fn parse_for_guild(contents: &str, guild_id: GuildId) -> Result<Self, ManagementError> {
        let raw: RawStateFile =
            serde_json::from_str(contents).map_err(|error| ManagementError::InvalidState(error.to_string()))?;
        raw.validate()
            .map_err(|error| ManagementError::InvalidState(error.to_string()))?;

        if raw.guild_id != guild_id {
            return Err(ManagementError::GuildMismatch {
                state_guild_id: raw.guild_id,
                actual_guild_id: guild_id,
            });
        }
        if raw.roles.values().any(|role_id| role_id.get() == guild_id.get()) {
            return Err(ManagementError::InvalidState(
                "予約参照 everyone の Role ID は state の別の論理 ID に対応付けできません".to_owned(),
            ));
        }

        Ok(Self {
            schema_version: raw.schema_version,
            guild_id: raw.guild_id,
            roles: raw.roles,
            channels: raw.channels,
            members: raw.members,
        })
    }
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

pub(crate) fn deserialize_unique_role_mappings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<RoleLogicalId, RoleId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_unique_mappings(deserializer, "Role")
}

pub(crate) fn deserialize_unique_channel_mappings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<ChannelLogicalId, ChannelId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_unique_mappings(deserializer, "Channel")
}

pub(crate) fn deserialize_unique_member_mappings<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<MemberLogicalId, MemberId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_unique_mappings(deserializer, "Member")
}

pub(crate) fn serialize_state(state: &StateFile) -> Result<String, ManagementError> {
    serde_json::to_string_pretty(state)
        .map(|json| format!("{json}\n"))
        .map_err(|error| ManagementError::SerializeState(error.to_string()))
}

pub(crate) fn validation_error(code: &'static str, message: impl Into<String>) -> ValidationError {
    ValidationError::new(code).with_message(message.into().into())
}

pub(crate) fn validate_role_mappings(roles: &BTreeMap<RoleLogicalId, RoleId>) -> Result<(), ValidationError> {
    if roles.contains_key(&everyone_logical_id()) {
        return Err(validation_error(
            "reserved_everyone_logical_id",
            "予約論理 ID everyone は state に含めず、definition だけで使用してください",
        ));
    }

    validate_unique_mappings(roles, "Role")
}

pub(crate) fn validate_channel_mappings(
    channels: &BTreeMap<ChannelLogicalId, ChannelId>,
) -> Result<(), ValidationError> {
    validate_unique_mappings(channels, "Channel")
}

pub(crate) fn validate_member_mappings(members: &BTreeMap<MemberLogicalId, MemberId>) -> Result<(), ValidationError> {
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

use super::*;
