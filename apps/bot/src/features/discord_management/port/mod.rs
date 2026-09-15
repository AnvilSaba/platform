//! Discord 管理の外部操作を表す Port です。
//!
//! 実装側（現在は Serenity Adapter）をこのモジュールの Interface に依存させ、
//! `export`・`plan`・`apply`・`bind` が Discord SDK の型を直接参照しないようにします。

use std::collections::{BTreeMap, BTreeSet};

use super::{
    configuration::{Color, KnownPermission},
    domain::{ManagementError, ResourceType},
    ids::{GuildId, RoleId},
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
