//! Role 適用 Workflow の入口です。
//!
//! Role 固有の状態機械・Discord 操作の組み立て・結果照合は
//! [`resource::role::apply`](super::resource::role::apply) に置き、ここでは
//! 利用者向けの結果型と依存 Port の受け渡しだけを扱います。

use std::time::Instant;

use super::{
    domain::ManagementError,
    port::{RoleLifecycleTarget, RoleUpdater},
    resource::role::{RolePlan, apply as role_apply},
};
use crate::features::discord_management::ids::GuildId;

pub(crate) use role_apply::{RoleApplyOptions, RoleApplyResult, RoleApplyStatus};

#[allow(dead_code)]
pub(crate) async fn apply_role_updates<S: RoleUpdater>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    role_apply::apply_role_updates(
        source,
        guild_id,
        definition_toml,
        state_json,
        confirmed_plan,
        processing_deadline,
    )
    .await
}

pub(super) async fn apply_roles<S: RoleLifecycleTarget>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    role_apply::apply_roles(
        source,
        guild_id,
        definition_toml,
        state_json,
        confirmed_plan,
        processing_deadline,
    )
    .await
}

pub(super) async fn apply_roles_with_options<S: RoleLifecycleTarget>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    options: RoleApplyOptions,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    role_apply::apply_roles_with_options(
        source,
        guild_id,
        definition_toml,
        state_json,
        confirmed_plan,
        options,
        processing_deadline,
    )
    .await
}
