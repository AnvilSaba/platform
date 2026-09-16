//! `export` ワークフローです。
//!
//! Discord の実構成を設定ファイルへ変換する責務だけを持ち、入力ファイルや
//! 差分適用の詳細は他の Feature に委譲します。

use std::collections::{BTreeMap, BTreeSet};

use super::{
    configuration::{
        ChannelKind, ChannelValue, ManagedValue, OverwriteValue, PermissionName, RawChannelAttributes,
        RawChannelDefinition, RawDefinitionFile, RawRoleAttributes, RawRoleDefinition, RawSettingsSets, RawStateFile,
        RoleEnsure, RoleMode, StateFile, everyone_logical_id,
    },
    domain::{ManagementError, SCHEMA_VERSION},
    ids::{ChannelId, ChannelLogicalId, GuildId, MemberId, RoleId, RoleLogicalId},
    port::{ChannelOverwritePermissions, ChannelOverwriteTarget, ChannelSource, RoleSource},
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

    let previous_mappings = previous_state
        .as_ref()
        .map(|state| state.roles.clone())
        .unwrap_or_default();
    let previous_logical_ids = previous_mappings
        .iter()
        .map(|(logical_id, discord_id)| (*discord_id, logical_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let catalog = source.role_catalog(&guild_id).await?;
    let catalog_ids = catalog.roles.iter().map(|role| role.id).collect::<BTreeSet<_>>();
    if previous_state.is_some() {
        for (logical_id, discord_id) in &previous_mappings {
            if catalog_ids.contains(discord_id) {
                continue;
            }
            return Err(ManagementError::InvalidState(format!(
                "論理 ID {logical_id} に対応する Role の Snowflake {discord_id} が Guild から予期せず消失しています"
            )));
        }
    }
    let roles = catalog.roles.into_iter().filter(|role| role.manageable);
    let mut definitions = BTreeMap::new();
    let mut mappings = previous_mappings.clone();

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
        if !is_everyone
            && let Some(existing_id) = mappings.insert(logical_id.clone(), role.id)
            && existing_id != role.id
        {
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
                    RawRoleAttributes {
                        permissions: role
                            .permissions
                            .into_iter()
                            .map(|(name, value)| {
                                (
                                    PermissionName::parse(name.as_str()).expect("Serenity の権限名は字句的に妥当です"),
                                    ManagedValue::Value(value),
                                )
                            })
                            .collect(),
                        ..RawRoleAttributes::default()
                    }
                } else {
                    RawRoleAttributes {
                        name: Some(ManagedValue::Value(role.name)),
                        color: Some(ManagedValue::Value(role.color)),
                        hoist: Some(ManagedValue::Value(role.hoist)),
                        mentionable: Some(ManagedValue::Value(role.mentionable)),
                        permissions: role
                            .permissions
                            .into_iter()
                            .filter(|(_, value)| *value)
                            .map(|(name, value)| {
                                (
                                    PermissionName::parse(name.as_str()).expect("Serenity の権限名は字句的に妥当です"),
                                    ManagedValue::Value(value),
                                )
                            })
                            .collect(),
                    }
                },
            },
        );
    }

    let definition_toml = serialize_definition(&RawDefinitionFile {
        schema_version: SCHEMA_VERSION,
        settings_sets: RawSettingsSets::default(),
        roles: definitions,
        channels: BTreeMap::new(),
        members: BTreeMap::new(),
        message_sets: BTreeMap::new(),
        threads: BTreeMap::new(),
        order: None,
    })?;
    let state_json = serde_json::to_string_pretty(&RawStateFile {
        schema_version: SCHEMA_VERSION,
        guild_id,
        roles: mappings,
        channels: previous_state
            .as_ref()
            .map(|state| state.channels.clone())
            .unwrap_or_default(),
        members: previous_state
            .as_ref()
            .map(|state| state.members.clone())
            .unwrap_or_default(),
    })
    .map_err(|error| ManagementError::SerializeState(error.to_string()))?;

    Ok(ExportFiles {
        definition_toml,
        state_json: format!("{state_json}\n"),
    })
}

/// 管理可能な Category/Text Channel の希望構成と対応 state を出力します。
pub(super) async fn export_channels<S: ChannelSource>(
    source: &S,
    guild_id: GuildId,
    previous_state_json: Option<&str>,
) -> Result<ExportFiles, ManagementError> {
    let previous_state = previous_state_json
        .map(|contents| StateFile::parse_for_guild(contents, guild_id))
        .transpose()?;
    let previous_mappings = previous_state
        .as_ref()
        .map(|state| state.channels.clone())
        .unwrap_or_default();
    let previous_logical_ids = previous_mappings
        .iter()
        .map(|(logical_id, discord_id)| (*discord_id, logical_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let catalog = source.channel_catalog(&guild_id).await?;
    let catalog_ids = catalog
        .channels
        .iter()
        .map(|channel| channel.id)
        .collect::<BTreeSet<_>>();
    if previous_state.is_some() {
        for (logical_id, discord_id) in &previous_mappings {
            if catalog_ids.contains(discord_id) {
                continue;
            }
            return Err(ManagementError::InvalidState(format!(
                "論理 ID {logical_id} に対応する Channel の Snowflake {discord_id} が Guild から予期せず消失しています"
            )));
        }
    }
    let channels = catalog
        .channels
        .iter()
        .filter(|channel| channel.manageable)
        .filter(|channel| matches!(channel.kind, ChannelKind::Category | ChannelKind::Text))
        .cloned()
        .collect::<Vec<_>>();

    let mut logical_ids = BTreeMap::<ChannelId, ChannelLogicalId>::new();
    let mut mappings = previous_mappings.clone();
    for channel in &channels {
        let logical_id = if let Some(logical_id) = previous_logical_ids.get(&channel.id) {
            logical_id.clone()
        } else {
            let generated = ChannelLogicalId::parse(format!("channel_{}", channel.id))
                .expect("Channel Snowflake から生成した論理 ID は常に有効です");
            if let Some(reserved_for) = previous_mappings.get(&generated) {
                return Err(ManagementError::InvalidState(format!(
                    "生成する論理 ID {generated} は state で Snowflake {reserved_for} に使用されています"
                )));
            }
            generated
        };
        if let Some(existing_id) = mappings.insert(logical_id.clone(), channel.id)
            && existing_id != channel.id
        {
            return Err(ManagementError::InvalidState(format!(
                "論理 ID {logical_id} が Channel {existing_id} と {} で衝突しています",
                channel.id
            )));
        }
        logical_ids.insert(channel.id, logical_id);
    }

    let previous_role_mappings = previous_state
        .as_ref()
        .map(|state| state.roles.clone())
        .unwrap_or_default();
    let mut role_ids = previous_role_mappings
        .iter()
        .map(|(logical_id, discord_id)| (*discord_id, logical_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let previous_member_mappings = previous_state
        .as_ref()
        .map(|state| state.members.clone())
        .unwrap_or_default();
    let mut member_ids = previous_member_mappings
        .iter()
        .map(|(logical_id, discord_id)| (*discord_id, logical_id.clone()))
        .collect::<BTreeMap<_, _>>();
    for channel in &channels {
        for target in channel.overwrites.keys() {
            match target {
                ChannelOverwriteTarget::Role(role_id) => {
                    register_role_overwrite_target(*role_id, &mut role_ids, &previous_role_mappings)?;
                }
                ChannelOverwriteTarget::Member(member_id) => {
                    register_member_overwrite_target(*member_id, &mut member_ids, &previous_member_mappings)?;
                }
                ChannelOverwriteTarget::Everyone => {}
            }
        }
    }

    let mut definitions = BTreeMap::new();
    let mut role_definitions = BTreeMap::new();
    for logical_id in role_ids.values() {
        role_definitions.insert(
            logical_id.clone(),
            super::configuration::RawRoleDefinition {
                ensure: super::configuration::RoleEnsure::Present,
                mode: super::configuration::RoleMode::Reference,
                settings_sets: Vec::new(),
                attributes: super::configuration::RawRoleAttributes::default(),
            },
        );
    }
    let mut member_definitions = BTreeMap::new();
    for logical_id in member_ids.values() {
        member_definitions.insert(
            logical_id.clone(),
            super::configuration::MemberDefinition {
                mode: super::configuration::RoleMode::Reference,
            },
        );
    }

    for channel in channels {
        let logical_id = logical_ids
            .get(&channel.id)
            .expect("論理 ID は先行する対応付けで生成されています")
            .clone();
        let kind = channel.kind;
        let mut attributes = RawChannelAttributes {
            kind: Some(kind.as_str().to_owned()),
            name: Some(ChannelValue::Value(channel.name)),
            ..RawChannelAttributes::default()
        };
        match channel.parent_id {
            Some(parent_id) => {
                let parent = logical_ids.get(&parent_id).ok_or_else(|| {
                    ManagementError::InvalidState(format!(
                        "Channel {logical_id} の親 Channel {parent_id} が export 対象に含まれていません"
                    ))
                })?;
                attributes.parent = Some(ChannelValue::Value(parent.clone()));
            }
            None if kind == ChannelKind::Text => {
                attributes.parent = Some(ChannelValue::Clear);
            }
            None => {}
        }
        if kind == ChannelKind::Text {
            match channel.topic {
                Some(topic) => {
                    attributes.topic = Some(ChannelValue::Value(topic));
                }
                None => {
                    attributes.topic = Some(ChannelValue::Clear);
                }
            }
            attributes.nsfw = Some(ChannelValue::Value(channel.nsfw));
            attributes.slowmode_seconds = Some(ChannelValue::Value(channel.slowmode_seconds));
            if let Some(minutes) = channel.default_auto_archive_minutes {
                attributes.default_auto_archive_minutes = Some(ChannelValue::Value(minutes));
            }
            attributes.default_thread_slowmode_seconds =
                Some(ChannelValue::Value(channel.default_thread_slowmode_seconds));
        }
        attributes.overwrites = export_overwrites(&channel.overwrites, &role_ids, &member_ids)?;
        definitions.insert(
            logical_id,
            RawChannelDefinition {
                mode: RoleMode::Managed,
                ensure: None,
                settings_sets: Vec::new(),
                attributes,
            },
        );
    }

    let definition_toml = serialize_definition(&RawDefinitionFile {
        schema_version: SCHEMA_VERSION,
        settings_sets: RawSettingsSets::default(),
        roles: role_definitions,
        channels: definitions,
        members: member_definitions,
        message_sets: BTreeMap::new(),
        threads: BTreeMap::new(),
        order: None,
    })?;
    let mut exported_roles = previous_role_mappings;
    for (discord_id, logical_id) in &role_ids {
        exported_roles.entry(logical_id.clone()).or_insert(*discord_id);
    }
    let mut exported_members = previous_member_mappings;
    for (discord_id, logical_id) in &member_ids {
        exported_members.entry(logical_id.clone()).or_insert(*discord_id);
    }
    let state_json = serde_json::to_string_pretty(&RawStateFile {
        schema_version: SCHEMA_VERSION,
        guild_id,
        roles: exported_roles,
        channels: mappings,
        members: exported_members,
    })
    .map_err(|error| ManagementError::SerializeState(error.to_string()))?;

    Ok(ExportFiles {
        definition_toml,
        state_json: format!("{state_json}\n"),
    })
}

fn serialize_definition(definition: &RawDefinitionFile) -> Result<String, ManagementError> {
    let serialized = toml::to_string_pretty(definition)
        .map_err(|error| ManagementError::SerializeDefinition(error.to_string()))?;
    let mut document = serialized
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| ManagementError::SerializeDefinition(error.to_string()))?;
    inline_marker_tables(document.as_item_mut());
    Ok(document.to_string())
}

fn inline_marker_tables(item: &mut toml_edit::Item) {
    match item {
        toml_edit::Item::Table(table) => {
            for (_, child) in table.iter_mut() {
                inline_marker_tables(child);
            }
            table.fmt();

            let marker = ["default", "clear"].into_iter().find(|key| {
                table.len() == 1 && table.get(key).and_then(toml_edit::Item::as_bool) == Some(true)
            });
            if let Some(marker) = marker {
                let mut inline = toml_edit::InlineTable::new();
                inline.insert(marker, true.into());
                *item = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
            }
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (_, child) in table.iter_mut() {
                    inline_marker_tables(child);
                }
            }
        }
        toml_edit::Item::None | toml_edit::Item::Value(_) => {}
    }
}

fn register_role_overwrite_target(
    discord_id: RoleId,
    role_ids: &mut BTreeMap<RoleId, RoleLogicalId>,
    previous_mappings: &BTreeMap<RoleLogicalId, RoleId>,
) -> Result<(), ManagementError> {
    if role_ids.contains_key(&discord_id) {
        return Ok(());
    }
    let logical_id =
        RoleLogicalId::parse(format!("role_{discord_id}")).expect("Role Snowflake から生成した論理 ID は常に有効です");
    if let Some(reserved_for) = previous_mappings.get(&logical_id) {
        return Err(ManagementError::InvalidState(format!(
            "生成する論理 ID {logical_id} は state で Snowflake {reserved_for} に使用されています"
        )));
    }
    role_ids.insert(discord_id, logical_id);
    Ok(())
}

fn register_member_overwrite_target(
    discord_id: MemberId,
    member_ids: &mut BTreeMap<MemberId, super::ids::MemberLogicalId>,
    previous_mappings: &BTreeMap<super::ids::MemberLogicalId, MemberId>,
) -> Result<(), ManagementError> {
    if member_ids.contains_key(&discord_id) {
        return Ok(());
    }
    let logical_id = super::ids::MemberLogicalId::parse(format!("member_{discord_id}"))
        .expect("Member Snowflake から生成した論理 ID は常に有効です");
    if let Some(reserved_for) = previous_mappings.get(&logical_id) {
        return Err(ManagementError::InvalidState(format!(
            "生成する論理 ID {logical_id} は state で Snowflake {reserved_for} に使用されています"
        )));
    }
    member_ids.insert(discord_id, logical_id);
    Ok(())
}

fn export_overwrites(
    overwrites: &BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>,
    role_ids: &BTreeMap<RoleId, RoleLogicalId>,
    member_ids: &BTreeMap<MemberId, super::ids::MemberLogicalId>,
) -> Result<BTreeMap<String, BTreeMap<PermissionName, OverwriteValue>>, ManagementError> {
    let mut result = BTreeMap::new();
    for (target, permissions) in overwrites {
        let subject = match target {
            ChannelOverwriteTarget::Everyone => "everyone".to_owned(),
            ChannelOverwriteTarget::Role(id) => {
                let logical_id = role_ids.get(id).ok_or_else(|| {
                    ManagementError::InvalidState(format!(
                        "Channel overwrite の Role {id} に対応する論理 ID が state にありません"
                    ))
                })?;
                format!("role:{logical_id}")
            }
            ChannelOverwriteTarget::Member(id) => {
                let logical_id = member_ids.get(id).ok_or_else(|| {
                    ManagementError::InvalidState(format!(
                        "Channel overwrite の Member {id} に対応する論理 ID が state にありません"
                    ))
                })?;
                format!("member:{logical_id}")
            }
        };
        let mut values = BTreeMap::new();
        for (permission, value) in &permissions.known {
            let permission =
                PermissionName::parse(permission.as_str().to_owned()).expect("Known permission は構文的に妥当です");
            values.insert(permission, *value);
        }
        result.insert(subject, values);
    }
    Ok(result)
}

#[cfg(test)]
mod marker_format_tests {
    use super::inline_marker_tables;

    #[test]
    fn marker_tables_are_inlined_without_inlining_structured_tables() {
        let mut document = r#"
[settings_sets.channels.base.topic]
default = true

[channels.rules.parent]
clear = true

[channels.rules.overwrites.everyone]
VIEW_CHANNEL = "deny"
"#
        .parse::<toml_edit::DocumentMut>()
        .unwrap();

        inline_marker_tables(document.as_item_mut());
        let output = document.to_string();

        assert!(output.contains("topic = { default = true }"));
        assert!(output.contains("parent = { clear = true }"));
        assert!(output.contains("[channels.rules.overwrites.everyone]"));
        assert!(!output.contains("overwrites = {"));
    }
}
