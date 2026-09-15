use super::*;

pub(crate) struct PlanInput {
    pub(crate) definition: DefinitionFile,
    pub(crate) state: StateFile,
}

impl PlanInput {
    pub(crate) fn parse(
        definition_toml: &str,
        state_json: &str,
        guild_id: GuildId,
        vocabulary: &PermissionVocabulary,
    ) -> Result<Self, ManagementError> {
        let definition = DefinitionFile::parse(definition_toml, vocabulary)?;
        let state = StateFile::parse_for_guild(state_json, guild_id)?;
        validate_references(&definition, &state)?;
        Ok(Self { definition, state })
    }
}

fn validate_references(definition: &DefinitionFile, state: &StateFile) -> Result<(), ManagementError> {
    for (logical_id, role) in &definition.roles {
        if *logical_id != everyone_logical_id() && role.is_reference() && !state.roles.contains_key(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の対応がありません"
            )));
        }
    }

    for (logical_id, channel) in &definition.channels {
        if channel.is_absent() {
            continue;
        }
        if !state.channels.contains_key(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の対応がありません"
            )));
        }
        validate_channel_references(logical_id, channel.attributes(), definition, state)?;
    }

    for (logical_id, member) in &definition.members {
        if !matches!(member.mode, RoleMode::Reference) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Member {logical_id} は参照専用として宣言してください"
            )));
        }
        if !state.members.contains_key(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Member {logical_id} の対応がありません"
            )));
        }
    }

    for (name, message_set) in &definition.message_sets {
        validate_channel_container_reference("管理メッセージ群", name, message_set, definition, state)?;
    }
    for (name, thread) in &definition.threads {
        validate_channel_container_reference("管理スレッド", name, thread, definition, state)?;
    }

    for (name, settings_set) in &definition.settings_sets.channel {
        validate_channel_references(
            &ChannelLogicalId::parse(format!("settings_set_{name}")).expect("設定セット検証用の論理 ID は常に有効です"),
            &settings_set.attributes,
            definition,
            state,
        )?;
    }

    Ok(())
}

fn validate_channel_container_reference(
    resource_kind: &str,
    resource_name: &str,
    value: &toml::Value,
    definition: &DefinitionFile,
    state: &StateFile,
) -> Result<(), ManagementError> {
    let Some(table) = value.as_table() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} はテーブルで指定してください"
        )));
    };
    if table
        .get("ensure")
        .and_then(toml::Value::as_str)
        .is_some_and(|ensure| ensure == "absent")
    {
        return Ok(());
    }

    let Some(channel) = table.get("channel") else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} の channel がありません"
        )));
    };
    let Some(channel) = channel.as_str() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} の channel は Channel 論理 ID で指定してください"
        )));
    };
    let channel_id = ChannelLogicalId::parse(channel).map_err(|error| {
        ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} の channel {channel} が不正です: {error}"
        ))
    })?;
    require_channel_reference(
        &channel_id,
        definition,
        state,
        &format!("{resource_kind} {resource_name} の投稿先"),
    )
}

fn validate_channel_references(
    channel_id: &ChannelLogicalId,
    attributes: &BTreeMap<String, toml::Value>,
    definition: &DefinitionFile,
    state: &StateFile,
) -> Result<(), ManagementError> {
    if let Some(parent) = attributes.get("parent") {
        if let Some(parent) = parent.as_str() {
            let parent_id = ChannelLogicalId::parse(parent).map_err(|error| {
                ManagementError::InvalidDefinition(format!("Channel {channel_id} の親 {parent} が不正です: {error}"))
            })?;
            require_channel_reference(&parent_id, definition, state, &format!("Channel {channel_id} の親"))?;
        } else if !is_clear_value(parent) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {channel_id} の parent は Channel 論理 ID または clear で指定してください"
            )));
        }
    }

    if let Some(overwrites) = attributes.get("overwrites") {
        let Some(overwrites) = overwrites.as_table() else {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {channel_id} の overwrites はテーブルで指定してください"
            )));
        };
        for subject in overwrites.keys() {
            if subject == "everyone" {
                continue;
            }
            if let Some(logical_id) = subject.strip_prefix("role:") {
                let logical_id = RoleLogicalId::parse(logical_id).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Channel {channel_id} の {subject} が不正です: {error}"))
                })?;
                if logical_id == everyone_logical_id() {
                    continue;
                }
                if !definition.roles.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidDefinition(format!(
                        "Channel {channel_id} の権限対象 Role {logical_id} の宣言がありません"
                    )));
                }
                if !state.roles.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidState(format!(
                        "Channel {channel_id} の権限対象 Role {logical_id} の対応がありません"
                    )));
                }
            } else if let Some(logical_id) = subject.strip_prefix("member:") {
                let logical_id = MemberLogicalId::parse(logical_id).map_err(|error| {
                    ManagementError::InvalidDefinition(format!("Channel {channel_id} の {subject} が不正です: {error}"))
                })?;
                if !definition.members.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidDefinition(format!(
                        "Channel {channel_id} の権限対象 Member {logical_id} の宣言がありません"
                    )));
                }
                if !state.members.contains_key(&logical_id) {
                    return Err(ManagementError::InvalidState(format!(
                        "Channel {channel_id} の権限対象 Member {logical_id} の対応がありません"
                    )));
                }
            } else {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {channel_id} の権限対象 {subject} は everyone、role:<論理 ID>、member:<論理 ID> のいずれかで指定してください"
                )));
            }
        }
    }

    Ok(())
}

fn require_channel_reference(
    logical_id: &ChannelLogicalId,
    definition: &DefinitionFile,
    state: &StateFile,
    context: &str,
) -> Result<(), ManagementError> {
    let Some(channel) = definition.channels.get(logical_id) else {
        return Err(ManagementError::InvalidDefinition(format!(
            "{context} Channel {logical_id} の宣言がありません"
        )));
    };
    if channel.is_absent() {
        return Err(ManagementError::InvalidDefinition(format!(
            "{context} Channel {logical_id} は削除宣言です"
        )));
    }
    if !state.channels.contains_key(logical_id) {
        return Err(ManagementError::InvalidState(format!(
            "{context} Channel {logical_id} の対応がありません"
        )));
    }
    Ok(())
}

fn is_clear_value(value: &toml::Value) -> bool {
    value
        .as_table()
        .and_then(|table| table.get("clear"))
        .and_then(toml::Value::as_bool)
        .is_some_and(|clear| clear)
}
