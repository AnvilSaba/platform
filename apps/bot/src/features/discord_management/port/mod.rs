//! Discord 管理の外部操作を表す Port です。
//!
//! 実装側（現在は Serenity Adapter）をこのモジュールの Interface に依存させ、
//! `export`・`plan`・`apply`・`bind` が Discord SDK の型を直接参照しないようにします。

use std::collections::{BTreeMap, BTreeSet};

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
    pub default_thread_slowmode_seconds: Option<u16>,
    pub overwrites: BTreeMap<ChannelOverwriteTarget, BTreeMap<KnownPermission, OverwriteValue>>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChannelCreate {
    pub kind: ChannelKind,
    pub name: String,
    pub parent_id: Option<ChannelId>,
    pub topic: Option<String>,
    pub nsfw: bool,
    pub slowmode_seconds: u16,
    pub default_auto_archive_minutes: Option<u16>,
    pub default_thread_slowmode_seconds: Option<u16>,
    pub overwrites: BTreeMap<ChannelOverwriteTarget, BTreeMap<KnownPermission, OverwriteValue>>,
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
    pub parent_id: Option<Option<ChannelId>>,
    pub topic: Option<Option<String>>,
    pub nsfw: Option<bool>,
    pub slowmode_seconds: Option<u16>,
    pub default_auto_archive_minutes: Option<Option<u16>>,
    pub default_thread_slowmode_seconds: Option<Option<u16>>,
    pub overwrites: Option<BTreeMap<ChannelOverwriteTarget, BTreeMap<KnownPermission, OverwriteValue>>>,
    /// Discord の `delete_permission` API で対象ごとに全解除する override です。
    pub permission_overwrites_to_delete: BTreeSet<ChannelOverwriteTarget>,
}

impl ChannelUpdate {
    pub(super) fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.parent_id.is_none()
            && self.topic.is_none()
            && self.nsfw.is_none()
            && self.slowmode_seconds.is_none()
            && self.default_auto_archive_minutes.is_none()
            && self.default_thread_slowmode_seconds.is_none()
            && self.overwrites.is_none()
            && self.permission_overwrites_to_delete.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn apply_to(&self, channel: &mut ChannelSnapshot) {
        if let Some(name) = &self.name {
            channel.name.clone_from(name);
        }
        if let Some(parent_id) = self.parent_id {
            channel.parent_id = parent_id;
        }
        if let Some(topic) = &self.topic {
            channel.topic.clone_from(topic);
        }
        if let Some(nsfw) = self.nsfw {
            channel.nsfw = nsfw;
        }
        if let Some(slowmode_seconds) = self.slowmode_seconds {
            channel.slowmode_seconds = slowmode_seconds;
        }
        if let Some(default_auto_archive_minutes) = self.default_auto_archive_minutes {
            channel.default_auto_archive_minutes = default_auto_archive_minutes;
        }
        if let Some(default_thread_slowmode_seconds) = self.default_thread_slowmode_seconds {
            channel.default_thread_slowmode_seconds = default_thread_slowmode_seconds;
        }
        if let Some(overwrites) = &self.overwrites {
            channel.overwrites.clone_from(overwrites);
        }
        for target in &self.permission_overwrites_to_delete {
            channel.overwrites.remove(target);
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
