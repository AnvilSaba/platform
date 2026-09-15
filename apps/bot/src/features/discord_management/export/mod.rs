//! `export` ワークフローです。
//!
//! Discord の実構成を設定ファイルへ変換する責務だけを持ち、入力ファイルや
//! 差分適用の詳細は他の Feature に委譲します。

use std::collections::{BTreeMap, BTreeSet};

use super::{
    configuration::{
        ManagedValue, RawDefinitionFile, RawRoleDefinition, RawStateFile, RoleAttributes, RoleEnsure, RoleMode,
        SettingsSets, StateFile, everyone_logical_id,
    },
    domain::{ManagementError, SCHEMA_VERSION},
    ids::{GuildId, RoleLogicalId},
    port::RoleSource,
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ExportFiles {
    pub definition_toml: String,
    pub state_json: String,
}

/// 管理可能な Role の希望構成と対応 state を出力します。
pub(super) async fn export_roles<S: RoleSource>(
    source: &S,
    guild_id: GuildId,
    previous_state_json: Option<&str>,
) -> Result<ExportFiles, ManagementError> {
    let previous_state = previous_state_json
        .map(|contents| StateFile::parse_for_guild(contents, guild_id))
        .transpose()?;

    let previous_mappings = previous_state.map(StateFile::into_role_mappings).unwrap_or_default();
    let previous_logical_ids = previous_mappings
        .iter()
        .map(|(logical_id, discord_id)| (*discord_id, logical_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let roles = source
        .role_catalog(&guild_id)
        .await?
        .roles
        .into_iter()
        .filter(|role| role.manageable);
    let mut definitions = BTreeMap::new();
    let mut mappings = BTreeMap::new();

    for role in roles {
        let is_everyone = role.id.get() == guild_id.get();
        let logical_id = if is_everyone {
            everyone_logical_id()
        } else if let Some(logical_id) = previous_logical_ids.get(&role.id) {
            logical_id.clone()
        } else {
            let generated = RoleLogicalId::parse(format!("role_{}", role.id))
                .expect("Role Snowflake から生成した論理 ID は常に有効です");
            if let Some(reserved_for) = previous_mappings.get(&generated) {
                return Err(ManagementError::InvalidState(format!(
                    "生成する論理 ID {generated} は state で Snowflake {reserved_for} に使用されています"
                )));
            }
            generated
        };
        if !is_everyone && let Some(existing_id) = mappings.insert(logical_id.clone(), role.id) {
            return Err(ManagementError::InvalidState(format!(
                "論理 ID {logical_id} が Role {existing_id} と {} で衝突しています",
                role.id
            )));
        }
        definitions.insert(
            logical_id.clone(),
            RawRoleDefinition {
                ensure: RoleEnsure::Present,
                mode: RoleMode::Managed,
                settings_sets: Vec::new(),
                attributes: if is_everyone {
                    RoleAttributes {
                        permissions: role
                            .permissions
                            .into_iter()
                            .map(|(name, value)| (name, ManagedValue::Value(value)))
                            .collect(),
                        ..RoleAttributes::default()
                    }
                } else {
                    RoleAttributes {
                        name: Some(ManagedValue::Value(role.name)),
                        color: Some(ManagedValue::Value(role.color)),
                        hoist: Some(ManagedValue::Value(role.hoist)),
                        mentionable: Some(ManagedValue::Value(role.mentionable)),
                        permissions: role
                            .permissions
                            .into_iter()
                            .filter(|(_, value)| *value)
                            .map(|(name, value)| (name, ManagedValue::Value(value)))
                            .collect(),
                    }
                },
            },
        );
    }

    let definition_toml = toml::to_string_pretty(&RawDefinitionFile {
        schema_version: SCHEMA_VERSION,
        settings_sets: SettingsSets::default(),
        roles: definitions,
        channels: BTreeMap::new(),
        members: BTreeMap::new(),
        message_sets: BTreeMap::new(),
        threads: BTreeMap::new(),
        order: None,
    })
    .map_err(|error| ManagementError::SerializeDefinition(error.to_string()))?;
    let state_json = serde_json::to_string_pretty(&RawStateFile {
        schema_version: SCHEMA_VERSION,
        guild_id,
        roles: mappings,
        channels: BTreeMap::new(),
        members: BTreeMap::new(),
        deleted_roles: BTreeSet::new(),
        pending_creations: BTreeSet::new(),
        pending_deletions: BTreeSet::new(),
    })
    .map_err(|error| ManagementError::SerializeState(error.to_string()))?;

    Ok(ExportFiles {
        definition_toml,
        state_json: format!("{state_json}\n"),
    })
}
