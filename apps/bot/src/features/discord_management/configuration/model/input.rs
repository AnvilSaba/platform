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
        if channel.is_reference() && !state.channels.contains_key(logical_id) {
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

    for (name, settings_set_attributes) in &definition.settings_sets.channel {
        validate_channel_references(
            &ChannelLogicalId::parse(format!("settings_set_{name}")).expect("設定セット検証用の論理 ID は常に有効です"),
            settings_set_attributes,
            definition,
            state,
        )?;
    }

    Ok(())
}

fn validate_channel_container_reference(
    resource_kind: &str,
    resource_name: &str,
    value: &RawResourceDefinition,
    definition: &DefinitionFile,
    state: &StateFile,
) -> Result<(), ManagementError> {
    if !value.is_table() {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} はテーブルで指定してください"
        )));
    }
    if value.is_absent() {
        return Ok(());
    }

    if !value.has_channel() {
        return Err(ManagementError::InvalidDefinition(format!(
            "{resource_kind} {resource_name} の channel がありません"
        )));
    }
    let Some(channel) = value.channel_name() else {
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
    attributes: &ChannelAttributes,
    definition: &DefinitionFile,
    state: &StateFile,
) -> Result<(), ManagementError> {
    if let Some(ChannelValue::Value(parent_id)) = &attributes.parent {
        require_channel_reference(parent_id, definition, state, &format!("Channel {channel_id} の親"))?;
    }

    for subject in attributes.overwrites.keys() {
        if subject == "everyone" {
            validate_role_overwrite_reference(channel_id, &everyone_logical_id(), definition, state)?;
            continue;
        }
        if let Some(logical_id) = subject.strip_prefix("role:") {
            let logical_id = RoleLogicalId::parse(logical_id).map_err(|error| {
                ManagementError::InvalidDefinition(format!("Channel {channel_id} の {subject} が不正です: {error}"))
            })?;
            if logical_id == everyone_logical_id() {
                validate_role_overwrite_reference(channel_id, &logical_id, definition, state)?;
                continue;
            }
            if !definition.roles.contains_key(&logical_id) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Channel {channel_id} の権限対象 Role {logical_id} の宣言がありません"
                )));
            }
            validate_role_overwrite_reference(channel_id, &logical_id, definition, state)?;
            if logical_id != everyone_logical_id() && !state.roles.contains_key(&logical_id) {
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

    Ok(())
}

fn validate_role_overwrite_reference(
    channel_id: &ChannelLogicalId,
    logical_id: &RoleLogicalId,
    definition: &DefinitionFile,
    state: &StateFile,
) -> Result<(), ManagementError> {
    if definition
        .roles
        .get(logical_id)
        .is_some_and(crate::features::discord_management::configuration::RoleDefinition::is_absent)
    {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {channel_id} の権限対象 Role {logical_id} は削除宣言です"
        )));
    }
    if state.deleted_roles.contains(logical_id) {
        return Err(ManagementError::InvalidState(format!(
            "Channel {channel_id} の権限対象 Role {logical_id} は削除済みです"
        )));
    }
    if state.pending_deletions.contains(logical_id) {
        return Err(ManagementError::InvalidState(format!(
            "Channel {channel_id} の権限対象 Role {logical_id} の削除意図が未解決です"
        )));
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
    if channel.is_reference() && !state.channels.contains_key(logical_id) {
        return Err(ManagementError::InvalidState(format!(
            "{context} Channel {logical_id} の対応がありません"
        )));
    }
    Ok(())
}
