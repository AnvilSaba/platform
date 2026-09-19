//! Discord 管理の外部操作を表す Port です。
//!
//! 実装側（現在は Serenity Adapter）をこのモジュールの Interface に依存させ、
//! `export`・`plan`・`apply`・`bind` が Discord SDK の型を直接参照しないようにします。

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use super::{
    configuration::{ChannelKind, Color, KnownPermission, OverwriteValue},
    domain::{ManagementError, ResourceType},
    ids::{ChannelId, GuildId, MemberId, RoleId},
};

/// Discord の Role 読み取り結果を表す Port DTO です。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RoleSnapshot {
    pub id: RoleId,
    pub manageable: bool,
    pub name: String,
    pub color: Color,
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: BTreeMap<KnownPermission, bool>,
}

/// Role の現在値と Discord 側で利用可能な権限をまとめた Port DTO です。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RoleCatalog {
    pub roles: Vec<RoleSnapshot>,
    pub permission_names: BTreeSet<KnownPermission>,
    pub grantable_permissions: BTreeSet<KnownPermission>,
    pub default_permissions: BTreeMap<KnownPermission, bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RoleCreate {
    pub name: String,
    pub color: Color,
    pub hoist: bool,
    pub mentionable: bool,
    pub permissions: BTreeMap<KnownPermission, bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoleCreateOutcome {
    Created(RoleId),
    ResponseUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoleDeleteOutcome {
    Deleted,
    ResponseUnknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RoleUpdate {
    pub name: Option<String>,
    pub color: Option<Color>,
    pub hoist: Option<bool>,
    pub mentionable: Option<bool>,
    pub permissions: Option<BTreeMap<KnownPermission, bool>>,
}

impl RoleUpdate {
    pub(super) fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.color.is_none()
            && self.hoist.is_none()
            && self.mentionable.is_none()
            && self.permissions.is_none()
    }

    #[cfg(test)]
    pub(crate) fn apply_to(&self, role: &mut RoleSnapshot) {
        if let Some(name) = &self.name {
            role.name.clone_from(name);
        }
        if let Some(color) = self.color {
            role.color = color;
        }
        if let Some(hoist) = self.hoist {
            role.hoist = hoist;
        }
        if let Some(mentionable) = self.mentionable {
            role.mentionable = mentionable;
        }
        if let Some(permissions) = &self.permissions {
            role.permissions.clone_from(permissions);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoleUpdateOutcome {
    Applied,
    ResponseUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ResourceLookup {
    pub resource_type: ResourceType,
    pub guild_id: GuildId,
}

/// Resource の存在と所属を検証するための Port です。
pub(super) trait ResourceSource {
    async fn lookup_resource(
        &self,
        guild_id: &GuildId,
        discord_id: u64,
    ) -> Result<Option<ResourceLookup>, ManagementError>;
}

/// Role の実構成を読み取るための Port です。
pub(super) trait RoleSource {
    async fn role_catalog(&self, guild_id: &GuildId) -> Result<RoleCatalog, ManagementError>;
}

/// Role の属性更新を行う Port です。
pub(super) trait RoleUpdater: RoleSource {
    async fn update_role(
        &self,
        guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError>;
}

/// Role の作成・削除を行う Port です。
pub(super) trait RoleLifecycleTarget: RoleUpdater {
    async fn create_role(&self, guild_id: &GuildId, create: RoleCreate) -> Result<RoleCreateOutcome, ManagementError>;

    async fn delete_role(&self, guild_id: &GuildId, role_id: &RoleId) -> Result<RoleDeleteOutcome, ManagementError>;
}

/// Category/Text Channel の現在値を表す Port DTO です。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChannelSnapshot {
    pub id: ChannelId,
    pub kind: ChannelKind,
    pub manageable: bool,
    pub name: String,
    pub parent_id: Option<ChannelId>,
    pub topic: Option<String>,
    pub nsfw: bool,
    pub slowmode_seconds: u16,
    pub default_auto_archive_minutes: Option<u16>,
    /// Discord が未設定を返す場合も API 上の canonical 値 0 として扱います。
    pub default_thread_slowmode_seconds: u16,
    pub overwrites: BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>,
}

/// Discord が追加した権限や、現在の SDK が名前を持たない権限 bit を保持します。
///
/// 管理設定から指定できる既知権限とは分離し、読み取った bit を更新時にもそのまま
/// 送り返せるようにします。値の意味は adapter に解釈させず、ここでは opaque な mask
/// として扱います。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PermissionBits(u64);

impl PermissionBits {
    pub(super) const fn new(bits: u64) -> Self {
        Self(bits)
    }

    pub(super) const fn bits(self) -> u64 {
        self.0
    }

    pub(super) const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// 一つの permission overwrite の既知部分と opaque な未知 bit です。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ChannelOverwritePermissions {
    pub known: BTreeMap<KnownPermission, OverwriteValue>,
    pub allow_unknown: PermissionBits,
    pub deny_unknown: PermissionBits,
}

impl ChannelOverwritePermissions {
    #[cfg(test)]
    pub(super) fn from_known(known: BTreeMap<KnownPermission, OverwriteValue>) -> Self {
        Self {
            known,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChannelCatalog {
    pub channels: Vec<ChannelSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ChannelOverwriteTarget {
    Everyone,
    Role(RoleId),
    Member(MemberId),
}

impl Ord for ChannelOverwriteTarget {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use ChannelOverwriteTarget::{Everyone, Member, Role};
        match (self, other) {
            (Everyone, Everyone) => std::cmp::Ordering::Equal,
            (Everyone, _) => std::cmp::Ordering::Less,
            (_, Everyone) => std::cmp::Ordering::Greater,
            (Role(left), Role(right)) => left.cmp(right),
            (Role(_), Member(_)) => std::cmp::Ordering::Less,
            (Member(_), Role(_)) => std::cmp::Ordering::Greater,
            (Member(left), Member(right)) => left.cmp(right),
        }
    }
}

impl PartialOrd for ChannelOverwriteTarget {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ChannelOverwriteTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Everyone => formatter.write_str("everyone"),
            Self::Role(id) => write!(formatter, "role:{id}"),
            Self::Member(id) => write!(formatter, "member:{id}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChannelCreate {
    pub kind: ChannelKind,
    pub name: String,
    pub parent_id: Option<ChannelId>,
    pub topic: Option<String>,
    pub nsfw: bool,
    pub slowmode_seconds: u16,
    pub default_auto_archive_minutes: Option<u16>,
    pub default_thread_slowmode_seconds: u16,
    pub overwrites: BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChannelCreateOutcome {
    Created(ChannelId),
    ResponseUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChannelDeleteOutcome {
    Deleted,
    ResponseUnknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ChannelUpdate {
    pub name: Option<String>,
    pub parent_id: ChannelUpdateValue<ChannelId>,
    pub topic: ChannelUpdateValue<String>,
    pub nsfw: Option<bool>,
    pub slowmode_seconds: Option<u16>,
    pub default_auto_archive_minutes: ChannelUpdateValue<u16>,
    /// `None` は Keep、具体的な値（解除時は 0）が更新値です。
    pub default_thread_slowmode_seconds: Option<u16>,
    pub overwrites: Option<BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>>,
}

/// Channel の nullable 属性を更新する要求です。
///
/// `Keep` は API payload から省略し、`Set` は具体値を設定し、`Clear` は
/// Discord の null 相当へ戻します。ネストした `Option` でこの三値を表現しません。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum ChannelUpdateValue<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

impl<T> ChannelUpdateValue<T> {
    pub(super) fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

impl ChannelUpdate {
    #[cfg(test)]
    pub(crate) fn apply_to(&self, channel: &mut ChannelSnapshot) {
        if let Some(name) = &self.name {
            channel.name.clone_from(name);
        }
        match &self.parent_id {
            ChannelUpdateValue::Keep => {}
            ChannelUpdateValue::Set(parent_id) => channel.parent_id = Some(*parent_id),
            ChannelUpdateValue::Clear => channel.parent_id = None,
        }
        match &self.topic {
            ChannelUpdateValue::Keep => {}
            ChannelUpdateValue::Set(topic) => channel.topic = Some(topic.clone()),
            ChannelUpdateValue::Clear => channel.topic = None,
        }
        if let Some(nsfw) = self.nsfw {
            channel.nsfw = nsfw;
        }
        if let Some(slowmode_seconds) = self.slowmode_seconds {
            channel.slowmode_seconds = slowmode_seconds;
        }
        match &self.default_auto_archive_minutes {
            ChannelUpdateValue::Keep => {}
            ChannelUpdateValue::Set(minutes) => channel.default_auto_archive_minutes = Some(*minutes),
            ChannelUpdateValue::Clear => channel.default_auto_archive_minutes = None,
        }
        if let Some(seconds) = self.default_thread_slowmode_seconds {
            channel.default_thread_slowmode_seconds = seconds;
        }
        if let Some(overwrites) = &self.overwrites {
            channel.overwrites.clone_from(overwrites);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChannelUpdateOutcome {
    Applied,
    ResponseUnknown,
}

/// Category/Text Channel の実構成を読み取るための Port です。
pub(super) trait ChannelSource {
    async fn channel_catalog(&self, guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError>;

    /// Channel の permission overwrite を更新できるかを事前に確認します。
    ///
    /// 既存の Port 実装は Channel catalog だけを提供しても動作できるよう、
    /// 既定値は許可とします。実環境の adapter は Discord の実権限を返します。
    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }
}

/// Category/Text Channel の属性更新を行う Port です。
pub(super) trait ChannelUpdater: ChannelSource {
    async fn update_channel(
        &self,
        guild_id: &GuildId,
        channel_id: &ChannelId,
        update: ChannelUpdate,
    ) -> Result<ChannelUpdateOutcome, ManagementError>;
}

/// Category/Text Channel の作成・削除を行う Port です。
pub(super) trait ChannelLifecycleTarget: ChannelUpdater {
    async fn create_channel(
        &self,
        guild_id: &GuildId,
        create: ChannelCreate,
    ) -> Result<ChannelCreateOutcome, ManagementError>;

    async fn delete_channel(
        &self,
        guild_id: &GuildId,
        channel_id: &ChannelId,
    ) -> Result<ChannelDeleteOutcome, ManagementError>;
}
