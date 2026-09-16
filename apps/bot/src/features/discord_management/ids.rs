use std::{fmt, marker::PhantomData, str::FromStr};

use nonmax::NonMaxU64;
use nutype::nutype;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

pub(crate) trait DiscordIdTag {
    const LABEL: &'static str;
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct DiscordId<Tag> {
    value: NonMaxU64,
    tag: PhantomData<fn() -> Tag>,
}

impl<Tag> DiscordId<Tag> {
    pub(crate) const fn new(value: u64) -> Self {
        match Self::try_new(value) {
            Some(id) => id,
            None => panic!("u64::MAX は Discord ID として使用できません"),
        }
    }

    const fn try_new(value: u64) -> Option<Self> {
        match NonMaxU64::new(value) {
            Some(value) => Some(Self {
                value,
                tag: PhantomData,
            }),
            None => None,
        }
    }

    pub(crate) const fn get(self) -> u64 {
        self.value.get()
    }
}

impl<Tag> fmt::Display for DiscordId<Tag> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.get().fmt(formatter)
    }
}

impl<Tag> FromStr for DiscordId<Tag> {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value
            .parse::<u64>()
            .map_err(|_| "Snowflake は整数文字列である必要があります")?;
        Self::try_new(value).ok_or("u64::MAX は Snowflake として使用できません")
    }
}

impl<Tag> Serialize for DiscordId<Tag> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, Tag: DiscordIdTag> Deserialize<'de> for DiscordId<Tag> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(|error| de::Error::custom(format_args!("{} が不正です: {error}", Tag::LABEL)))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) enum GuildIdTag {}

impl DiscordIdTag for GuildIdTag {
    const LABEL: &'static str = "Guild ID";
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) enum RoleIdTag {}

impl DiscordIdTag for RoleIdTag {
    const LABEL: &'static str = "Role ID";
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) enum ChannelIdTag {}

impl DiscordIdTag for ChannelIdTag {
    const LABEL: &'static str = "Channel ID";
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub(crate) enum MemberIdTag {}

impl DiscordIdTag for MemberIdTag {
    const LABEL: &'static str = "Member ID";
}

pub(crate) type GuildId = DiscordId<GuildIdTag>;
pub(crate) type RoleId = DiscordId<RoleIdTag>;
pub(crate) type ChannelId = DiscordId<ChannelIdTag>;
pub(crate) type MemberId = DiscordId<MemberIdTag>;

#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
struct TaggedLogicalId<Tag> {
    value: String,
    #[serde(skip)]
    tag: PhantomData<fn() -> Tag>,
}

impl<Tag> TaggedLogicalId<Tag> {
    fn new(value: String) -> Self {
        Self {
            value,
            tag: PhantomData,
        }
    }
}

impl<Tag> fmt::Display for TaggedLogicalId<Tag> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LogicalIdError;

impl fmt::Display for LogicalIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("論理 ID は英数字、ハイフン、アンダースコアだけで指定してください")
    }
}

impl std::error::Error for LogicalIdError {}

fn validate_logical_id<Tag>(logical_id: &TaggedLogicalId<Tag>) -> Result<(), LogicalIdError> {
    let value = logical_id.value.as_bytes();
    if !value.is_empty()
        && value
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(LogicalIdError)
    }
}

#[nutype(
    validate(with = validate_logical_id, error = LogicalIdError),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Serialize, Deserialize)
)]
pub(crate) struct LogicalId<Tag>(TaggedLogicalId<Tag>);

impl<Tag> LogicalId<Tag> {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, LogicalIdError> {
        Self::try_new(TaggedLogicalId::new(value.into()))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize)]
pub(crate) enum RoleLogicalIdTag {}

impl fmt::Display for RoleLogicalIdTag {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize)]
pub(crate) enum RoleSettingsSetIdTag {}

impl fmt::Display for RoleSettingsSetIdTag {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

pub(crate) type RoleLogicalId = LogicalId<RoleLogicalIdTag>;
pub(crate) type RoleSettingsSetId = LogicalId<RoleSettingsSetIdTag>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize)]
pub(crate) enum ChannelLogicalIdTag {}

impl fmt::Display for ChannelLogicalIdTag {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize)]
pub(crate) enum MemberLogicalIdTag {}

impl fmt::Display for MemberLogicalIdTag {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize)]
pub(crate) enum MessageLogicalIdTag {}

impl fmt::Display for MessageLogicalIdTag {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord, Serialize)]
pub(crate) enum ChannelSettingsSetIdTag {}

impl fmt::Display for ChannelSettingsSetIdTag {
    fn fmt(&self, _formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

pub(crate) type ChannelLogicalId = LogicalId<ChannelLogicalIdTag>;
pub(crate) type MemberLogicalId = LogicalId<MemberLogicalIdTag>;
pub(crate) type MessageLogicalId = LogicalId<MessageLogicalIdTag>;
pub(crate) type ChannelSettingsSetId = LogicalId<ChannelSettingsSetIdTag>;
