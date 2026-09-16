use std::collections::BTreeMap;

use serde::{Serialize, Serializer};
use serenity::{
    Error as SerenityError,
    all::{
        AutoArchiveDuration, ChannelId as SerenityChannelId, ChannelType, GuildId as SerenityGuildId,
        PermissionOverwrite, PermissionOverwriteType, Permissions, UserId,
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
        ChannelOverwritePermissions, ChannelOverwriteTarget, ChannelSnapshot, ChannelSource, ChannelUpdate,
        ChannelUpdateOutcome, ChannelUpdateValue, ChannelUpdater, PermissionBits,
    },
};

use super::{resource::SerenityManagementAdapter, role::permission_vocabulary};

#[cfg(test)]
use serenity::all::RoleId as SerenityRoleId;

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

/// Discord の snowflake は JSON 上では文字列として送ります。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiscordSnowflake(u64);

impl Serialize for DiscordSnowflake {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

/// Discord API の channel type 値です。enum の意味を adapter 内に閉じ込めます。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiscordChannelType {
    Text,
    Category,
}

impl Serialize for DiscordChannelType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(match self {
            Self::Text => 0,
            Self::Category => 4,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct DiscordSeconds(u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
struct DiscordMinutes(u16);

/// Discord の permission bitfield は JSON 上では文字列として送ります。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiscordPermissionBits(u64);

impl Serialize for DiscordPermissionBits {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiscordPermissionOverwriteType {
    Role,
    Member,
}

impl Serialize for DiscordPermissionOverwriteType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(match self {
            Self::Role => 0,
            Self::Member => 1,
        })
    }
}

#[derive(Debug, Serialize)]
struct DiscordPermissionOverwrite {
    id: DiscordSnowflake,
    #[serde(rename = "type")]
    kind: DiscordPermissionOverwriteType,
    allow: DiscordPermissionBits,
    deny: DiscordPermissionBits,
}

/// Discord の nullable な PATCH 属性を表します。
///
/// `Keep` はフィールド自体を省略し、`Clear` は JSON null を送ります。
#[derive(Debug)]
enum NullablePatchField<T> {
    Keep,
    Set(T),
    Clear,
}

impl<T> NullablePatchField<T> {
    fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

impl<T: Serialize> Serialize for NullablePatchField<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Keep => serializer.serialize_unit(),
            Self::Set(value) => value.serialize(serializer),
            Self::Clear => serializer.serialize_none(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum CreateChannelRequest {
    Text(CreateTextChannelRequest),
    Category(CreateCategoryChannelRequest),
}

#[derive(Debug, Serialize)]
struct CreateTextChannelRequest {
    name: String,
    #[serde(rename = "type")]
    channel_type: DiscordChannelType,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<DiscordSnowflake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    nsfw: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_per_user: Option<DiscordSeconds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_auto_archive_duration: Option<DiscordMinutes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_thread_rate_limit_per_user: Option<DiscordSeconds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_overwrites: Option<Vec<DiscordPermissionOverwrite>>,
}

#[derive(Debug, Serialize)]
struct CreateCategoryChannelRequest {
    name: String,
    #[serde(rename = "type")]
    channel_type: DiscordChannelType,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_overwrites: Option<Vec<DiscordPermissionOverwrite>>,
}

#[derive(Debug, Serialize)]
struct ModifyChannelRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "NullablePatchField::is_keep")]
    parent_id: NullablePatchField<DiscordSnowflake>,
    #[serde(skip_serializing_if = "NullablePatchField::is_keep")]
    topic: NullablePatchField<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nsfw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit_per_user: Option<DiscordSeconds>,
    #[serde(skip_serializing_if = "NullablePatchField::is_keep")]
    default_auto_archive_duration: NullablePatchField<DiscordMinutes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_thread_rate_limit_per_user: Option<DiscordSeconds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_overwrites: Option<Vec<DiscordPermissionOverwrite>>,
}

impl ChannelSource for SerenityManagementAdapter<'_> {
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

async fn bot_permissions(
    source: &SerenityManagementAdapter<'_>,
    guild_id: &GuildId,
) -> Result<Permissions, ManagementError> {
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
    let known_permission_mask = known_permission_mask(known_permissions);
    let overwrites = channel
        .permission_overwrites
        .iter()
        .filter_map(|overwrite| overwrite_snapshot(overwrite, guild_id, known_permissions, known_permission_mask))
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
    known_permission_mask: Permissions,
) -> Option<Result<(ChannelOverwriteTarget, ChannelOverwritePermissions), ManagementError>> {
    let target = match overwrite.kind {
        PermissionOverwriteType::Role(role_id) if role_id.get() == guild_id.get() => ChannelOverwriteTarget::Everyone,
        PermissionOverwriteType::Role(role_id) => ChannelOverwriteTarget::Role(RoleId::new(role_id.get())),
        PermissionOverwriteType::Member(user_id) => ChannelOverwriteTarget::Member(MemberId::new(user_id.get())),
        _ => return None,
    };
    let known = known_permissions
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
    let allow_unknown = PermissionBits::new(overwrite.allow.bits() & !known_permission_mask.bits());
    let deny_unknown = PermissionBits::new(overwrite.deny.bits() & !known_permission_mask.bits());
    Some(Ok((
        target,
        ChannelOverwritePermissions {
            known,
            allow_unknown,
            deny_unknown,
        },
    )))
}

fn known_permission_mask(known_permissions: &[KnownPermission]) -> Permissions {
    known_permissions.iter().fold(Permissions::empty(), |mask, permission| {
        mask | super::role::serenity_permission(permission)
    })
}

impl ChannelUpdater for SerenityManagementAdapter<'_> {
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
        || !update.parent_id.is_keep()
        || !update.topic.is_keep()
        || update.nsfw.is_some()
        || update.slowmode_seconds.is_some()
        || !update.default_auto_archive_minutes.is_keep()
        || !update.default_thread_slowmode_seconds.is_keep()
        || update.overwrites.is_some()
}

fn channel_update_requires_manage_roles(update: &ChannelUpdate) -> bool {
    update.overwrites.is_some()
}

fn has_manage_roles(permissions: Permissions) -> bool {
    permissions.contains(Permissions::MANAGE_ROLES) || permissions.contains(Permissions::ADMINISTRATOR)
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

impl ChannelLifecycleTarget for SerenityManagementAdapter<'_> {
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

fn create_channel_payload(guild_id: &GuildId, create: ChannelCreate) -> CreateChannelRequest {
    let permission_overwrites = (!create.overwrites.is_empty())
        .then(|| overwrite_payloads(guild_id, &create.overwrites))
        .filter(|overwrites| !overwrites.is_empty());
    match create.kind {
        ChannelKind::Text => CreateChannelRequest::Text(CreateTextChannelRequest {
            name: create.name,
            channel_type: DiscordChannelType::Text,
            parent_id: create.parent_id.map(|id| DiscordSnowflake(id.get())),
            topic: create.topic,
            nsfw: create.nsfw,
            rate_limit_per_user: Some(DiscordSeconds(create.slowmode_seconds)),
            default_auto_archive_duration: create.default_auto_archive_minutes.map(DiscordMinutes),
            default_thread_rate_limit_per_user: create.default_thread_slowmode_seconds.map(DiscordSeconds),
            permission_overwrites,
        }),
        ChannelKind::Category => CreateChannelRequest::Category(CreateCategoryChannelRequest {
            name: create.name,
            channel_type: DiscordChannelType::Category,
            permission_overwrites,
        }),
        ChannelKind::Unsupported => unreachable!("Unsupported Channel は構成管理から作成できません"),
    }
}

fn edit_channel_payload(guild_id: &GuildId, update: &ChannelUpdate) -> ModifyChannelRequest {
    ModifyChannelRequest {
        name: update.name.clone(),
        parent_id: nullable_patch_field(&update.parent_id, |id| DiscordSnowflake(id.get())),
        topic: nullable_patch_field(&update.topic, Clone::clone),
        nsfw: update.nsfw,
        rate_limit_per_user: update.slowmode_seconds.map(DiscordSeconds),
        default_auto_archive_duration: nullable_patch_field(&update.default_auto_archive_minutes, |minutes| {
            DiscordMinutes(*minutes)
        }),
        default_thread_rate_limit_per_user: match &update.default_thread_slowmode_seconds {
            ChannelUpdateValue::Keep => None,
            ChannelUpdateValue::Set(seconds) => Some(DiscordSeconds(*seconds)),
            // Discord の仕様上、この属性を無効化する値は 0 です。
            ChannelUpdateValue::Clear => Some(DiscordSeconds(0)),
        },
        permission_overwrites: update
            .overwrites
            .as_ref()
            .map(|overwrites| overwrite_payloads(guild_id, overwrites)),
    }
}

fn nullable_patch_field<T, U>(value: &ChannelUpdateValue<T>, map: impl FnOnce(&T) -> U) -> NullablePatchField<U> {
    match value {
        ChannelUpdateValue::Keep => NullablePatchField::Keep,
        ChannelUpdateValue::Set(value) => NullablePatchField::Set(map(value)),
        ChannelUpdateValue::Clear => NullablePatchField::Clear,
    }
}

fn overwrite_payloads(
    guild_id: &GuildId,
    overwrites: &BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>,
) -> Vec<DiscordPermissionOverwrite> {
    overwrites
        .iter()
        .filter_map(|(target, permissions)| {
            let mut allow = Permissions::from_bits_retain(permissions.allow_unknown.bits());
            let mut deny = Permissions::from_bits_retain(permissions.deny_unknown.bits());
            for (permission, value) in &permissions.known {
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
                ChannelOverwriteTarget::Everyone => (guild_id.get(), DiscordPermissionOverwriteType::Role),
                ChannelOverwriteTarget::Role(id) => (id.get(), DiscordPermissionOverwriteType::Role),
                ChannelOverwriteTarget::Member(id) => (id.get(), DiscordPermissionOverwriteType::Member),
            };
            Some(DiscordPermissionOverwrite {
                id: DiscordSnowflake(id),
                kind,
                allow: DiscordPermissionBits(allow.bits()),
                deny: DiscordPermissionBits(deny.bits()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_json<T: Serialize>(payload: &T) -> serde_json::Value {
        serde_json::to_value(payload).expect("payload は必ず JSON に直列化できます")
    }

    fn expected_json(value: &str) -> serde_json::Value {
        serde_json::from_str(value).expect("テストの JSON literal は妥当です")
    }

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
            ChannelOverwritePermissions::from_known(BTreeMap::from([(permission, OverwriteValue::Deny)])),
        );
        let payload = edit_channel_payload(
            &GuildId::new(100),
            &ChannelUpdate {
                parent_id: ChannelUpdateValue::Clear,
                topic: ChannelUpdateValue::Clear,
                overwrites: Some(overwrites),
                ..ChannelUpdate::default()
            },
        );

        assert_eq!(
            payload_json(&payload),
            expected_json(
                r#"{
                    "parent_id": null,
                    "topic": null,
                    "permission_overwrites": [{
                        "id": "100",
                        "type": 0,
                        "allow": "0",
                        "deny": "1024"
                    }]
                }"#,
            )
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

        assert_eq!(
            payload_json(&payload),
            expected_json(
                r#"{
                    "name": "rules",
                    "type": 0,
                    "parent_id": "200",
                    "topic": "案内",
                    "nsfw": false,
                    "rate_limit_per_user": 5,
                    "default_auto_archive_duration": 4320,
                    "default_thread_rate_limit_per_user": 10
                }"#,
            )
        );
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
                    ChannelOverwritePermissions::from_known(BTreeMap::from([(permission, OverwriteValue::Clear)])),
                )]),
            },
        );

        assert_eq!(
            payload_json(&payload),
            expected_json(
                r#"{
            "name": "rules",
            "type": 0,
            "nsfw": false,
            "rate_limit_per_user": 0
        }"#
            )
        );
    }

    #[test]
    fn edit_payload_uses_null_for_optional_values_and_category_omits_text_attributes() {
        let payload = edit_channel_payload(
            &GuildId::new(100),
            &ChannelUpdate {
                nsfw: Some(true),
                slowmode_seconds: Some(0),
                default_auto_archive_minutes: ChannelUpdateValue::Clear,
                default_thread_slowmode_seconds: ChannelUpdateValue::Clear,
                ..ChannelUpdate::default()
            },
        );
        assert_eq!(
            payload_json(&payload),
            expected_json(
                r#"{
                    "nsfw": true,
                    "rate_limit_per_user": 0,
                    "default_auto_archive_duration": null,
                    "default_thread_rate_limit_per_user": 0
                }"#,
            )
        );

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
        assert_eq!(
            payload_json(&category),
            expected_json(
                r#"{
            "name": "案内",
            "type": 4
        }"#
            )
        );
    }

    #[test]
    fn channel_snapshot_keeps_permission_bits_unknown_to_the_vocabulary() {
        let permission = permission_vocabulary()
            .known_permissions()
            .find(|permission| permission.as_str() == "VIEW_CHANNEL")
            .expect("Serenity の権限語彙に VIEW_CHANNEL が含まれます");
        let known = super::super::role::serenity_permission(&permission);
        let unknown_allow = 1_u64 << 60;
        let unknown_deny = 1_u64 << 61;
        let overwrite = PermissionOverwrite {
            allow: known | Permissions::from_bits_retain(unknown_allow),
            deny: Permissions::from_bits_retain(unknown_deny),
            kind: PermissionOverwriteType::Role(SerenityRoleId::new(400)),
        };

        let (_, permissions) = overwrite_snapshot(
            &overwrite,
            &GuildId::new(100),
            std::slice::from_ref(&permission),
            known_permission_mask(std::slice::from_ref(&permission)),
        )
        .expect("Role overwrite は snapshot 対象です")
        .expect("Role overwrite の target は解決できます");

        assert_eq!(permissions.known, BTreeMap::from([(permission, OverwriteValue::Allow)]));
        assert_eq!(permissions.allow_unknown, PermissionBits::new(unknown_allow));
        assert_eq!(permissions.deny_unknown, PermissionBits::new(unknown_deny));
    }

    #[test]
    fn edit_payload_merges_unknown_bits_and_serializes_every_overwrite_target() {
        let view_channel = permission_vocabulary()
            .known_permissions()
            .find(|permission| permission.as_str() == "VIEW_CHANNEL")
            .expect("Serenity の権限語彙に VIEW_CHANNEL が含まれます");
        let send_messages = permission_vocabulary()
            .known_permissions()
            .find(|permission| permission.as_str() == "SEND_MESSAGES")
            .expect("Serenity の権限語彙に SEND_MESSAGES が含まれます");
        let mut overwrites = BTreeMap::new();
        overwrites.insert(
            ChannelOverwriteTarget::Everyone,
            ChannelOverwritePermissions {
                known: BTreeMap::from([(view_channel.clone(), OverwriteValue::Allow)]),
                allow_unknown: PermissionBits::new(1_u64 << 60),
                deny_unknown: PermissionBits::new(1_u64 << 61),
            },
        );
        overwrites.insert(
            ChannelOverwriteTarget::Role(RoleId::new(200)),
            ChannelOverwritePermissions {
                known: BTreeMap::new(),
                allow_unknown: PermissionBits::new(1_u64 << 62),
                deny_unknown: PermissionBits::default(),
            },
        );
        overwrites.insert(
            ChannelOverwriteTarget::Member(MemberId::new(300)),
            ChannelOverwritePermissions {
                known: BTreeMap::from([(send_messages, OverwriteValue::Deny)]),
                allow_unknown: PermissionBits::default(),
                deny_unknown: PermissionBits::new(1_u64 << 60),
            },
        );

        let payload = edit_channel_payload(
            &GuildId::new(100),
            &ChannelUpdate {
                overwrites: Some(overwrites),
                ..ChannelUpdate::default()
            },
        );

        assert_eq!(
            payload_json(&payload),
            expected_json(
                r#"{
                    "permission_overwrites": [
                        {
                            "id": "100",
                            "type": 0,
                            "allow": "1152921504606848000",
                            "deny": "2305843009213693952"
                        },
                        {
                            "id": "200",
                            "type": 0,
                            "allow": "4611686018427387904",
                            "deny": "0"
                        },
                        {
                            "id": "300",
                            "type": 1,
                            "allow": "0",
                            "deny": "1152921504606849024"
                        }
                    ]
                }"#,
            )
        );
    }

    #[test]
    fn empty_permission_overwrites_are_sent_as_a_full_replacement() {
        let update = ChannelUpdate {
            overwrites: Some(BTreeMap::new()),
            ..ChannelUpdate::default()
        };

        assert!(channel_edit_is_required(&update));
        let payload = edit_channel_payload(&GuildId::new(100), &update);
        assert_eq!(
            payload_json(&payload),
            expected_json(
                r#"{
            "permission_overwrites": []
        }"#
            )
        );
    }
}
