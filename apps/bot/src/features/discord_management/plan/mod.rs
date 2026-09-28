//! `plan` ワークフローです。
//!
//! 入力の構文・Guild 対象を検証してから、各管理対象リソースの純粋な差分計算を
//! 呼び出します。現在は Role の差分計算を実装し、Channel・管理メッセージ等の
//! 宣言は共有 configuration で検証できる状態を保ちます。

use std::collections::BTreeSet;

use super::{
    configuration::{PermissionVocabulary, PlanInput},
    domain::ManagementError,
    ids::{MemberId, RoleId},
    port::RoleSource,
    resource::role::{RolePlan, build_plan},
};

use super::port::ChannelSource;
use super::resource::channel::{ChannelPlan, build_channel_plan_with_capabilities};

/// 希望構成と実構成を比較し、全体管理計画の Role 部分を返します。
pub(super) async fn plan_roles<S: RoleSource>(
    source: &S,
    vocabulary: &PermissionVocabulary,
    guild_id: super::ids::GuildId,
    definition_toml: &str,
    state_json: &str,
) -> Result<RolePlan, ManagementError> {
    let input = PlanInput::parse(definition_toml, state_json, guild_id, vocabulary)?;
    let catalog = source.role_catalog(&guild_id).await?;
    build_plan(&input.definition, &input.state, &catalog)
}

/// 希望構成と実構成を比較し、Category/Text Channel 部分の変更計画を返します。
pub(super) async fn plan_channels<S: ChannelSource>(
    source: &S,
    vocabulary: &PermissionVocabulary,
    guild_id: super::ids::GuildId,
    definition_toml: &str,
    state_json: &str,
) -> Result<ChannelPlan, ManagementError> {
    let input = PlanInput::parse(definition_toml, state_json, guild_id, vocabulary)?;
    let (role_ids, member_ids) = channel_permission_target_ids(&input.definition, &input.state);
    source
        .validate_channel_permission_targets(&guild_id, &role_ids, &member_ids)
        .await?;
    let catalog = source.channel_catalog(&guild_id).await?;
    let can_manage_roles = source.can_manage_roles(&guild_id).await?;
    build_channel_plan_with_capabilities(&input.definition, &input.state, &catalog, can_manage_roles)
}

pub(crate) fn channel_permission_target_ids(
    definition: &super::configuration::DefinitionFile,
    state: &super::configuration::StateFile,
) -> (Vec<RoleId>, Vec<MemberId>) {
    let mut role_ids = BTreeSet::new();
    let mut member_ids = BTreeSet::new();
    for channel in definition.channels.values() {
        for target in channel.attributes().overwrites.keys() {
            match target {
                super::configuration::OverwriteTarget::Everyone => {}
                super::configuration::OverwriteTarget::Role(logical_id) => {
                    if let Some(discord_id) = state.roles.get(logical_id).copied() {
                        role_ids.insert(discord_id);
                    }
                }
                super::configuration::OverwriteTarget::Member(logical_id) => {
                    if let Some(discord_id) = state.members.get(logical_id).copied() {
                        member_ids.insert(discord_id);
                    }
                }
            }
        }
    }
    (role_ids.into_iter().collect(), member_ids.into_iter().collect())
}
