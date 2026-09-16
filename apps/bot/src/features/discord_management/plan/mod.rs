//! `plan` ワークフローです。
//!
//! 入力の構文・Guild 対象を検証してから、各管理対象リソースの純粋な差分計算を
//! 呼び出します。現在は Role の差分計算を実装し、Channel・管理メッセージ等の
//! 宣言は共有 configuration で検証できる状態を保ちます。

use super::{
    configuration::{PermissionVocabulary, PlanInput},
    domain::ManagementError,
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
    let catalog = source.channel_catalog(&guild_id).await?;
    let can_manage_roles = source.can_manage_roles(&guild_id).await?;
    build_channel_plan_with_capabilities(&input.definition, &input.state, &catalog, can_manage_roles)
}
