use std::{fmt, marker::PhantomData, str::FromStr};

use nonmax::NonMaxU64;
use nutype::nutype;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

macro_rules! snowflake_id {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
        #[repr(transparent)]
        pub(crate) struct $name(NonMaxU64);

        impl $name {
            pub(crate) const fn new(value: u64) -> Option<Self> {
                match NonMaxU64::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            pub(crate) const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.get().fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = &'static str;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let value = value
                    .parse::<u64>()
                    .map_err(|_| "Snowflake は整数文字列である必要があります")?;
                Self::new(value).ok_or("u64::MAX は Snowflake として使用できません")
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(|error| de::Error::custom(format_args!(concat!($label, " が不正です: {}"), error)))
            }
        }
    };
}

snowflake_id!(GuildSnowflake, "Guild ID");
snowflake_id!(RoleSnowflake, "Role ID");

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snowflake_newtypes_preserve_the_nonmax_niche() {
        assert_eq!(
            std::mem::size_of::<Option<GuildSnowflake>>(),
            std::mem::size_of::<GuildSnowflake>()
        );
        assert_eq!(
            std::mem::size_of::<Option<RoleSnowflake>>(),
            std::mem::size_of::<RoleSnowflake>()
        );
    }
}
