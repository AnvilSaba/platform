//! Discord 管理の適用 Workflow の入口です。
//!
//! Guild 単位の排他状態と、Role 固有の適用処理・Discord 操作をまとめた
//! Workflow を公開します。

pub(super) mod guild_lock;

use super::resource::role::apply as role_apply;

pub(crate) use guild_lock::GuildApplyLock;
pub(crate) use role_apply::{RoleApplyOptions, RoleApplyResult, RoleApplyStatus, RoleApplyWorkflow};
