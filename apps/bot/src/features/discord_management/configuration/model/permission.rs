use std::{borrow::Borrow, collections::BTreeSet, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// 設定ファイル上で字句的に妥当な権限名です。
///
/// Discord/Serenity の具体的な語彙には依存せず、設定の構文段階でのみ使います。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct PermissionName(String);

impl PermissionName {
    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || !value.bytes().enumerate().all(|(index, byte)| {
                (index == 0 && byte.is_ascii_uppercase())
                    || (index > 0 && (byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
            })
        {
            return Err(format!("不正な権限名 {value} が指定されています"));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PermissionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PermissionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

impl Serialize for PermissionName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// 構成へ解決済みの権限名です。
///
/// `PermissionVocabulary` による解決を通過した値だけがこの型になります。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct KnownPermission(String);

impl KnownPermission {
    fn from_name(name: &PermissionName) -> Self {
        Self(name.as_str().to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for KnownPermission {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for KnownPermission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 実行環境が解決できる権限語彙です。
///
/// Serenity などの Adapter が現在のSDK語彙から構築し、configuration parserへ
/// 注入します。Guildごとの付与可能性や既定値は含めず、名前の解決だけを担います。
#[derive(Clone, Debug, Default)]
pub(crate) struct PermissionVocabulary {
    names: BTreeSet<PermissionName>,
}

impl PermissionVocabulary {
    pub(crate) fn from_names<I, S>(names: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        names
            .into_iter()
            .map(|name| PermissionName::parse(name))
            .collect::<Result<BTreeSet<_>, _>>()
            .map(|names| Self { names })
    }

    pub(crate) fn resolve(&self, name: &PermissionName) -> Option<KnownPermission> {
        self.names.get(name).map(KnownPermission::from_name)
    }

    pub(crate) fn known_permissions(&self) -> impl Iterator<Item = KnownPermission> + '_ {
        self.names.iter().map(KnownPermission::from_name)
    }
}
