use nutype::nutype;
use serde::{Deserialize, Serialize};

use super::{ChannelValue, Ensure, RoleMode};
use crate::features::discord_management::{domain::ManagementError, ids::EmojiId};

/// ファイルの入力形式。解析後の処理へは渡しません。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTagDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ensure: Option<Ensure>,
    #[serde(default, skip_serializing_if = "RoleMode::is_managed")]
    pub(crate) mode: RoleMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<ChannelValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) moderated: Option<ChannelValue<bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) emoji: Option<ChannelValue<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TagDefinition {
    Managed { attributes: TagAttributes },
    Reference,
    Absent,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TagAttributes {
    pub(crate) name: Option<TagName>,
    pub(crate) moderated: Option<bool>,
    pub(crate) emoji: Option<TagEmojiUpdate>,
}

#[nutype(
    validate(predicate = |name: &str| (1..=20).contains(&name.chars().count())),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub(crate) struct TagName(String);

#[nutype(
    validate(predicate = |emoji: &str| (1..=100).contains(&emoji.chars().count())
        && !emoji.bytes().all(|byte| byte.is_ascii_digit())),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub(crate) struct TagUnicodeEmoji(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TagEmoji {
    Custom(EmojiId),
    Unicode(TagUnicodeEmoji),
}

impl TagEmoji {
    fn parse(value: String) -> Result<Self, ManagementError> {
        let invalid = || ManagementError::InvalidDefinition("Tag emoji の値または custom emoji ID が不正です".into());
        if value.bytes().all(|byte| byte.is_ascii_digit()) {
            value.parse::<EmojiId>().map(Self::Custom).map_err(|_| invalid())
        } else {
            TagUnicodeEmoji::try_new(value)
                .map(Self::Unicode)
                .map_err(|_| invalid())
        }
    }
    pub(crate) fn into_string(self) -> String {
        match self {
            Self::Custom(id) => id.to_string(),
            Self::Unicode(value) => value.into_inner(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TagEmojiUpdate {
    Set(TagEmoji),
    Clear,
}

impl TagDefinition {
    pub(super) fn parse(raw: RawTagDefinition) -> Result<Self, ManagementError> {
        let has_attributes = raw.name.is_some() || raw.moderated.is_some() || raw.emoji.is_some();
        match (raw.ensure, raw.mode) {
            (Some(Ensure::Absent), RoleMode::Managed) if !has_attributes => Ok(Self::Absent),
            (Some(Ensure::Absent), _) => Err(ManagementError::InvalidDefinition(
                "削除 Tag には mode や管理属性を指定できません".into(),
            )),
            (None, RoleMode::Reference) if !has_attributes => Ok(Self::Reference),
            (_, RoleMode::Reference) => Err(ManagementError::InvalidDefinition(
                "参照専用 Tag には ensure や管理属性を指定できません".into(),
            )),
            (None | Some(Ensure::Present), RoleMode::Managed) => {
                let name = raw
                    .name
                    .map(|name| match name {
                        ChannelValue::Value(name) => TagName::try_new(name).map_err(|_| {
                            ManagementError::InvalidDefinition(
                                "Tag name は1文字以上20文字以内の値で指定してください".into(),
                            )
                        }),
                        ChannelValue::Default | ChannelValue::Clear => Err(ManagementError::InvalidDefinition(
                            "Tag name は1文字以上20文字以内の値で指定してください".into(),
                        )),
                    })
                    .transpose()?;
                let moderated = raw
                    .moderated
                    .map(|moderated| match moderated {
                        ChannelValue::Value(value) => Ok(value),
                        ChannelValue::Default => Ok(false),
                        ChannelValue::Clear => Err(ManagementError::InvalidDefinition(
                            "Tag moderated は clear を指定できません".into(),
                        )),
                    })
                    .transpose()?;
                let emoji = raw
                    .emoji
                    .map(|emoji| match emoji {
                        ChannelValue::Value(value) => TagEmoji::parse(value).map(TagEmojiUpdate::Set),
                        ChannelValue::Default | ChannelValue::Clear => Ok(TagEmojiUpdate::Clear),
                    })
                    .transpose()?;
                Ok(Self::Managed {
                    attributes: TagAttributes { name, moderated, emoji },
                })
            }
        }
    }
    pub(crate) fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}
