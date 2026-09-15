use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use serenity::{
    Error as SerenityError,
    all::{
        AutoArchiveDuration, ChannelId as SerenityChannelId, ChannelType, GuildId as SerenityGuildId,
        PermissionOverwrite, PermissionOverwriteType, Permissions, RoleId as SerenityRoleId, UserId,
    },
    http::{HttpError, StatusCode},
    model::channel::GuildChannel,
};

use crate::features::discord_management::{
    configuration::{ChannelKind, KnownPermission, OverwriteValue},
    domain::ManagementError,
    ids::{ChannelId, GuildId, MemberId, RoleId},
    port::{
        ChannelCatalog, ChannelCreate, ChannelCreateOutcome, ChannelDeleteOutcome, ChannelLifecycleTarget,
        ChannelOverwriteTarget, ChannelSnapshot, ChannelSource, ChannelUpdate, ChannelUpdateOutcome, ChannelUpdater,
    },
};

use super::{resource::SerenityRoleSource, role::permission_vocabulary};

impl From<SerenityChannelId> for ChannelId {
    fn from(id: SerenityChannelId) -> Self {
        Self::new(id.get())
    }
}

impl From<ChannelId> for SerenityChannelId {
    fn from(id: ChannelId) -> Self {
        Self::new(id.get())
    }
}

impl From<UserId> for MemberId {
    fn from(id: UserId) -> Self {
        Self::new(id.get())
    }
}

impl From<MemberId> for UserId {
    fn from(id: MemberId) -> Self {
        Self::new(id.get())
    }
}

impl ChannelSource for SerenityRoleSource<'_> {
    async fn channel_catalog(&self, guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        let serenity_guild_id = SerenityGuildId::from(*guild_id);
        let channels = serenity_guild_id
            .channels(self.http)
            .await
            .map_err(map_channel_catalog_error)?;
        let bot_permissions = bot_permissions(self, guild_id).await?;
        if !bot_permissions.intersects(Permissions::MANAGE_CHANNELS | Permissions::ADMINISTRATOR) {
            return Err(ManagementError::ChannelCatalogPermissionDenied(
                "Bot に MANAGE_CHANNELS 権限がありません".to_owned(),
            ));
        }

        let vocabulary = permission_vocabulary();
        let known_permissions = vocabulary.known_permissions().collect::<Vec<_>>();
        let manageable = bot_permissions.contains(Permissions::ADMINISTRATOR)
            || bot_permissions.contains(Permissions::MANAGE_CHANNELS);
        let channels = channels
            .into_iter()
            .map(|channel| channel_snapshot(&channel, guild_id, &known_permissions, manageable))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ChannelCatalog { channels })
    }

    async fn can_manage_roles(&self, guild_id: &GuildId) -> Result<bool, ManagementError> {
        let bot_permissions = bot_permissions(self, guild_id).await?;
        Ok(has_manage_roles(bot_permissions))
    }
}

async fn bot_permissions(source: &SerenityRoleSource<'_>, guild_id: &GuildId) -> Result<Permissions, ManagementError> {
    let serenity_guild_id = SerenityGuildId::from(*guild_id);
    let roles = serenity_guild_id
        .roles(source.http)
        .await
        .map_err(map_channel_catalog_error)?;
    let bot_member = serenity_guild_id
        .member(source.http, source.bot_user_id)
        .await
        .map_err(map_channel_catalog_error)?;
    let everyone_id = serenity_guild_id.everyone_role();
    let mut permissions = roles.get(&everyone_id).map(|role| role.permissions).unwrap_or_default();
    for role_id in &bot_member.roles {
        if let Some(role) = roles.get(role_id) {
            permissions |= role.permissions;
        }
    }
    Ok(permissions)
}

fn map_channel_catalog_error(error: SerenityError) -> ManagementError {
    match error {
        SerenityError::Http(error) if error.status_code() == Some(StatusCode::FORBIDDEN) => {
            ManagementError::ChannelCatalogPermissionDenied(error.to_string())
        }
        error => ManagementError::ChannelSource(error.to_string()),
    }
}

fn channel_snapshot(
    channel: &GuildChannel,
    guild_id: &GuildId,
    known_permissions: &[KnownPermission],
    manageable: bool,
) -> Result<ChannelSnapshot, ManagementError> {
    let kind = match channel.base.kind {
        ChannelType::Category => ChannelKind::Category,
        ChannelType::Text => ChannelKind::Text,
        _ => ChannelKind::Unsupported,
    };
    let overwrites = channel
        .permission_overwrites
        .iter()
        .filter_map(|overwrite| overwrite_snapshot(overwrite, guild_id, known_permissions))
        .collect::<Result<BTreeMap<_, _>, _>>();
    overwrites.map(|overwrites| ChannelSnapshot {
        id: ChannelId::from(channel.id),
        kind,
        manageable,
        name: channel.base.name.to_string(),
        parent_id: channel.parent_id.map(ChannelId::from),
        topic: channel.topic.as_ref().map(ToString::to_string),
        nsfw: channel.nsfw,
        slowmode_seconds: channel.base.rate_limit_per_user.map_or(0, |seconds| seconds.get()),
        default_auto_archive_minutes: channel.default_auto_archive_duration.and_then(auto_archive_minutes),
        default_thread_slowmode_seconds: channel.default_thread_rate_limit_per_user.map(|seconds| seconds.get()),
        overwrites,
    })
}

fn auto_archive_minutes(duration: AutoArchiveDuration) -> Option<u16> {
    match duration {
        AutoArchiveDuration::OneHour => Some(60),
        AutoArchiveDuration::OneDay => Some(1440),
        AutoArchiveDuration::ThreeDays => Some(4320),
        AutoArchiveDuration::OneWeek => Some(10080),
        _ => None,
    }
}

fn overwrite_snapshot(
    overwrite: &PermissionOverwrite,
    guild_id: &GuildId,
    known_permissions: &[KnownPermission],
) -> Option<Result<(ChannelOverwriteTarget, BTreeMap<KnownPermission, OverwriteValue>), ManagementError>> {
    let target = match overwrite.kind {
        PermissionOverwriteType::Role(role_id) if role_id.get() == guild_id.get() => ChannelOverwriteTarget::Everyone,
        PermissionOverwriteType::Role(role_id) => ChannelOverwriteTarget::Role(RoleId::new(role_id.get())),
        PermissionOverwriteType::Member(user_id) => ChannelOverwriteTarget::Member(MemberId::new(user_id.get())),
        _ => return None,
    };
    let permissions = known_permissions
        .iter()
        .filter_map(|permission| {
            let serenity_permission = super::role::serenity_permission(permission);
            if overwrite.allow.contains(serenity_permission) {
                Some((permission.clone(), OverwriteValue::Allow))
            } else if overwrite.deny.contains(serenity_permission) {
                Some((permission.clone(), OverwriteValue::Deny))
            } else {
                None
            }
        })
        .collect::<BTreeMap<_, _>>();
    Some(Ok((target, permissions)))
}

impl ChannelUpdater for SerenityRoleSource<'_> {
    async fn update_channel(
        &self,
        guild_id: &GuildId,
        channel_id: &ChannelId,
        update: ChannelUpdate,
    ) -> Result<ChannelUpdateOutcome, ManagementError> {
        if channel_update_requires_manage_roles(&update) && !has_manage_roles(bot_permissions(self, guild_id).await?) {
            return Err(ManagementError::ChannelPermissionDenied(
                "Channel の permission overwrite 更新には MANAGE_ROLES 権限が必要です".to_owned(),
            ));
        }
        let serenity_channel_id = SerenityChannelId::from(*channel_id);
        // Channel 編集の `permission_overwrites` は配列全体の置換です。後続の置換で
        // 削除対象が先に消えてしまわないよう、delete_permission を先に実行します。
        for target in &update.permission_overwrites_to_delete {
            let permission_type = permission_overwrite_type(guild_id, target);
            match serenity_channel_id
                .delete_permission(self.http, permission_type, None)
                .await
            {
                Ok(()) => {}
                Err(error) => return map_channel_update_error(error),
            }
        }

        if channel_edit_is_required(&update) {
            let payload = edit_channel_payload(guild_id, &update);
            match self.http.edit_channel(serenity_channel_id.into(), &payload, None).await {
                Ok(_) => {}
                Err(error) => return map_channel_update_error(error),
            }
        }
        Ok(ChannelUpdateOutcome::Applied)
    }
}

fn channel_edit_is_required(update: &ChannelUpdate) -> bool {
    update.name.is_some()
        || update.parent_id.is_some()
        || update.topic.is_some()
        || update.nsfw.is_some()
        || update.slowmode_seconds.is_some()
        || update.default_auto_archive_minutes.is_some()
        || update.default_thread_slowmode_seconds.is_some()
        || update
            .overwrites
            .as_ref()
            .is_some_and(|overwrites| !overwrites.is_empty())
}

fn channel_update_requires_manage_roles(update: &ChannelUpdate) -> bool {
    !update.permission_overwrites_to_delete.is_empty()
        || update
            .overwrites
            .as_ref()
            .is_some_and(|overwrites| !overwrites.is_empty())
}

fn has_manage_roles(permissions: Permissions) -> bool {
    permissions.contains(Permissions::MANAGE_ROLES) || permissions.contains(Permissions::ADMINISTRATOR)
}

fn permission_overwrite_type(guild_id: &GuildId, target: &ChannelOverwriteTarget) -> PermissionOverwriteType {
    match target {
        ChannelOverwriteTarget::Everyone => {
            PermissionOverwriteType::Role(SerenityGuildId::from(*guild_id).everyone_role())
        }
        ChannelOverwriteTarget::Role(role_id) => PermissionOverwriteType::Role(SerenityRoleId::new(role_id.get())),
        ChannelOverwriteTarget::Member(member_id) => PermissionOverwriteType::Member(UserId::new(member_id.get())),
    }
}

fn map_channel_update_error(error: SerenityError) -> Result<ChannelUpdateOutcome, ManagementError> {
    match error {
        SerenityError::Io(_) | SerenityError::Http(HttpError::Request(_)) => Ok(ChannelUpdateOutcome::ResponseUnknown),
        SerenityError::Http(error) if error.status_code() == Some(StatusCode::FORBIDDEN) => {
            Err(ManagementError::ChannelPermissionDenied(error.to_string()))
        }
        error => Err(ManagementError::ChannelSource(error.to_string())),
    }
}

impl ChannelLifecycleTarget for SerenityRoleSource<'_> {
    async fn create_channel(
        &self,
        guild_id: &GuildId,
        create: ChannelCreate,
    ) -> Result<ChannelCreateOutcome, ManagementError> {
        let payload = create_channel_payload(guild_id, create);
        match self
            .http
            .create_channel(SerenityGuildId::from(*guild_id), &payload, None)
            .await
        {
            Ok(channel) => Ok(ChannelCreateOutcome::Created(ChannelId::from(channel.id))),
            Err(SerenityError::Io(_)) | Err(SerenityError::Http(HttpError::Request(_))) => {
                Ok(ChannelCreateOutcome::ResponseUnknown)
            }
            Err(SerenityError::Http(error)) if error.status_code() == Some(StatusCode::FORBIDDEN) => {
                Err(ManagementError::ChannelPermissionDenied(error.to_string()))
            }
            Err(error) => Err(ManagementError::ChannelSource(error.to_string())),
        }
    }

    async fn delete_channel(
        &self,
        _guild_id: &GuildId,
        channel_id: &ChannelId,
    ) -> Result<ChannelDeleteOutcome, ManagementError> {
        match self
            .http
            .delete_channel(SerenityChannelId::from(*channel_id).into(), None)
            .await
        {
            Ok(_) => Ok(ChannelDeleteOutcome::Deleted),
            Err(SerenityError::Io(_)) | Err(SerenityError::Http(HttpError::Request(_))) => {
                Ok(ChannelDeleteOutcome::ResponseUnknown)
            }
            Err(SerenityError::Http(error)) if error.status_code() == Some(StatusCode::FORBIDDEN) => {
                Err(ManagementError::ChannelPermissionDenied(error.to_string()))
            }
            Err(error) => Err(ManagementError::ChannelSource(error.to_string())),
        }
    }
}

fn create_channel_payload(guild_id: &GuildId, create: ChannelCreate) -> Value {
    let mut payload = Map::new();
    payload.insert("name".to_owned(), Value::String(create.name));
    payload.insert(
        "type".to_owned(),
        Value::Number(serde_json::Number::from(match create.kind {
            ChannelKind::Text => 0,
            ChannelKind::Category => 4,
            ChannelKind::Unsupported => unreachable!("Unsupported Channel は構成管理から作成できません"),
        })),
    );
    if let Some(parent_id) = create.parent_id {
        payload.insert("parent_id".to_owned(), json!(parent_id.get()));
    }
    if create.kind == ChannelKind::Text {
        if let Some(topic) = create.topic {
            payload.insert("topic".to_owned(), Value::String(topic));
        }
        payload.insert("nsfw".to_owned(), Value::Bool(create.nsfw));
        payload.insert(
            "rate_limit_per_user".to_owned(),
            Value::Number(serde_json::Number::from(create.slowmode_seconds)),
        );
        if let Some(minutes) = create.default_auto_archive_minutes {
            payload.insert("default_auto_archive_duration".to_owned(), json!(minutes));
        }
        if let Some(seconds) = create.default_thread_slowmode_seconds {
            payload.insert("default_thread_rate_limit_per_user".to_owned(), json!(seconds));
        }
    } else {
        payload.insert("nsfw".to_owned(), Value::Bool(create.nsfw));
    }
    if !create.overwrites.is_empty() {
        let overwrites = overwrite_payloads(guild_id, &create.overwrites);
        if !overwrites.as_array().is_some_and(Vec::is_empty) {
            payload.insert("permission_overwrites".to_owned(), overwrites);
        }
    }
    Value::Object(payload)
}

fn edit_channel_payload(guild_id: &GuildId, update: &ChannelUpdate) -> Value {
    let mut payload = Map::new();
    if let Some(name) = &update.name {
        payload.insert("name".to_owned(), Value::String(name.clone()));
    }
    if let Some(parent_id) = update.parent_id {
        payload.insert(
            "parent_id".to_owned(),
            parent_id.map_or(Value::Null, |id| json!(id.get())),
        );
    }
    if let Some(topic) = &update.topic {
        payload.insert("topic".to_owned(), topic.clone().map_or(Value::Null, Value::String));
    }
    if let Some(nsfw) = update.nsfw {
        payload.insert("nsfw".to_owned(), Value::Bool(nsfw));
    }
    if let Some(seconds) = update.slowmode_seconds {
        payload.insert("rate_limit_per_user".to_owned(), json!(seconds));
    }
    if let Some(minutes) = update.default_auto_archive_minutes {
        payload.insert(
            "default_auto_archive_duration".to_owned(),
            minutes.map_or(Value::Null, |minutes| json!(minutes)),
        );
    }
    if let Some(seconds) = update.default_thread_slowmode_seconds {
        payload.insert(
            "default_thread_rate_limit_per_user".to_owned(),
            seconds.map_or(Value::Null, |seconds| json!(seconds)),
        );
    }
    if let Some(overwrites) = &update.overwrites {
        payload.insert(
            "permission_overwrites".to_owned(),
            overwrite_payloads(guild_id, overwrites),
        );
    }
    Value::Object(payload)
}

fn overwrite_payloads(
    guild_id: &GuildId,
    overwrites: &BTreeMap<ChannelOverwriteTarget, BTreeMap<KnownPermission, OverwriteValue>>,
) -> Value {
    Value::Array(
        overwrites
            .iter()
            .filter_map(|(target, permissions)| {
                let mut allow = Permissions::empty();
                let mut deny = Permissions::empty();
                for (permission, value) in permissions {
                    match value {
                        OverwriteValue::Allow => allow |= super::role::serenity_permission(permission),
                        OverwriteValue::Deny => deny |= super::role::serenity_permission(permission),
                        OverwriteValue::Clear => {}
                    }
                }
                if allow.is_empty() && deny.is_empty() {
                    return None;
                }
                let (id, kind) = match target {
                    ChannelOverwriteTarget::Everyone => (guild_id.get(), 0),
                    ChannelOverwriteTarget::Role(id) => (id.get(), 0),
                    ChannelOverwriteTarget::Member(id) => (id.get(), 1),
                };
                Some(json!({
                    "id": id,
                    "type": kind,
                    "allow": allow.bits().to_string(),
                    "deny": deny.bits().to_string(),
                }))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manage_roles_precheck_accepts_manage_roles_or_administrator() {
        assert!(has_manage_roles(Permissions::MANAGE_ROLES));
        assert!(has_manage_roles(Permissions::ADMINISTRATOR));
        assert!(!has_manage_roles(Permissions::MANAGE_CHANNELS));
    }

    #[test]
    fn edit_payload_uses_null_for_parent_and_topic_clear_and_full_overwrite_array() {
        let vocabulary =
            crate::features::discord_management::configuration::PermissionVocabulary::from_names(["VIEW_CHANNEL"])
                .unwrap();
        let permission = vocabulary.known_permissions().next().unwrap();
        let mut overwrites = BTreeMap::new();
        overwrites.insert(
            ChannelOverwriteTarget::Everyone,
            BTreeMap::from([(permission, OverwriteValue::Deny)]),
        );
        let payload = edit_channel_payload(
            &GuildId::new(100),
            &ChannelUpdate {
                parent_id: Some(None),
                topic: Some(None),
                overwrites: Some(overwrites),
                ..ChannelUpdate::default()
            },
        );

        assert_eq!(payload["parent_id"], Value::Null);
        assert_eq!(payload["topic"], Value::Null);
        assert_eq!(payload["permission_overwrites"][0]["id"], json!(100));
        assert_eq!(payload["permission_overwrites"][0]["type"], json!(0));
        assert_eq!(payload["permission_overwrites"][0]["allow"], json!("0"));
        assert_eq!(
            payload["permission_overwrites"][0]["deny"],
            json!(Permissions::VIEW_CHANNEL.bits().to_string())
        );
    }

    #[test]
    fn create_payload_contains_explicit_channel_type_and_text_defaults() {
        let payload = create_channel_payload(
            &GuildId::new(100),
            ChannelCreate {
                kind: ChannelKind::Text,
                name: "rules".to_owned(),
                parent_id: Some(ChannelId::new(200)),
                topic: Some("案内".to_owned()),
                nsfw: false,
                slowmode_seconds: 5,
                default_auto_archive_minutes: Some(4320),
                default_thread_slowmode_seconds: Some(10),
                overwrites: BTreeMap::new(),
            },
        );

        assert_eq!(payload["type"], json!(0));
        assert_eq!(payload["parent_id"], json!(200));
        assert_eq!(payload["topic"], json!("案内"));
        assert_eq!(payload["rate_limit_per_user"], json!(5));
        assert_eq!(payload["default_auto_archive_duration"], json!(4320));
        assert_eq!(payload["default_thread_rate_limit_per_user"], json!(10));
    }

    #[test]
    fn create_payload_omits_overwrite_entries_that_only_clear_permissions() {
        let permission = permission_vocabulary()
            .known_permissions()
            .find(|permission| permission.as_str() == "VIEW_CHANNEL")
            .expect("Serenity の権限語彙に VIEW_CHANNEL が含まれます");
        let payload = create_channel_payload(
            &GuildId::new(100),
            ChannelCreate {
                kind: ChannelKind::Text,
                name: "rules".to_owned(),
                parent_id: None,
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: None,
                default_thread_slowmode_seconds: None,
                overwrites: BTreeMap::from([(
                    ChannelOverwriteTarget::Everyone,
                    BTreeMap::from([(permission, OverwriteValue::Clear)]),
                )]),
            },
        );

        assert!(payload.get("permission_overwrites").is_none());
    }

    #[test]
    fn edit_payload_uses_null_for_optional_thread_defaults_and_category_sends_nsfw() {
        let payload = edit_channel_payload(
            &GuildId::new(100),
            &ChannelUpdate {
                nsfw: Some(true),
                slowmode_seconds: Some(0),
                default_auto_archive_minutes: Some(None),
                default_thread_slowmode_seconds: Some(None),
                ..ChannelUpdate::default()
            },
        );
        assert_eq!(payload["nsfw"], json!(true));
        assert_eq!(payload["rate_limit_per_user"], json!(0));
        assert_eq!(payload["default_auto_archive_duration"], Value::Null);
        assert_eq!(payload["default_thread_rate_limit_per_user"], Value::Null);

        let category = create_channel_payload(
            &GuildId::new(100),
            ChannelCreate {
                kind: ChannelKind::Category,
                name: "案内".to_owned(),
                parent_id: None,
                topic: None,
                nsfw: true,
                slowmode_seconds: 0,
                default_auto_archive_minutes: None,
                default_thread_slowmode_seconds: None,
                overwrites: BTreeMap::new(),
            },
        );
        assert_eq!(category["type"], json!(4));
        assert_eq!(category["nsfw"], json!(true));
        assert!(category.get("rate_limit_per_user").is_none());
    }

    #[test]
    fn permission_overwrite_delete_targets_map_to_discord_types() {
        let guild_id = GuildId::new(100);
        assert_eq!(
            permission_overwrite_type(&guild_id, &ChannelOverwriteTarget::Everyone),
            PermissionOverwriteType::Role(SerenityRoleId::new(100))
        );
        assert_eq!(
            permission_overwrite_type(&guild_id, &ChannelOverwriteTarget::Role(RoleId::new(200))),
            PermissionOverwriteType::Role(SerenityRoleId::new(200))
        );
        assert_eq!(
            permission_overwrite_type(&guild_id, &ChannelOverwriteTarget::Member(MemberId::new(300))),
            PermissionOverwriteType::Member(UserId::new(300))
        );
    }

    #[test]
    fn all_clear_update_skips_empty_permission_overwrites_patch() {
        let update = ChannelUpdate {
            overwrites: Some(BTreeMap::new()),
            permission_overwrites_to_delete: std::collections::BTreeSet::from([ChannelOverwriteTarget::Everyone]),
            ..ChannelUpdate::default()
        };

        assert!(!channel_edit_is_required(&update));
    }
}
