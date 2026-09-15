use serenity::{
    Error as SerenityError,
    all::{Colour, EditRole, GuildId as SerenityGuildId, Permissions, RoleId as SerenityRoleId},
    http::{HttpError, StatusCode},
};

use crate::features::discord_management::configuration::{Color, KnownPermission, PermissionVocabulary};
use crate::features::discord_management::domain::ManagementError;
use crate::features::discord_management::ids::{GuildId, RoleId};
use crate::features::discord_management::port::{
    RoleCatalog, RoleCreate, RoleCreateOutcome, RoleDeleteOutcome, RoleLifecycleTarget, RoleSnapshot, RoleSource,
    RoleUpdate, RoleUpdateOutcome, RoleUpdater,
};

use super::resource::SerenityRoleSource;

pub(crate) fn permission_vocabulary() -> PermissionVocabulary {
    PermissionVocabulary::from_names(Permissions::all().iter_names().map(|(name, _)| name.to_owned()))
        .expect("Serenity の権限名は字句的に妥当です")
}

pub(super) fn serenity_permission(permission: &KnownPermission) -> Permissions {
    Permissions::all()
        .iter_names()
        .find_map(|(known_name, permission_value)| (known_name == permission.as_str()).then_some(permission_value))
        .expect("PermissionVocabulary と Serenity の権限語彙が一致しています")
}

impl From<SerenityRoleId> for RoleId {
    fn from(id: SerenityRoleId) -> Self {
        Self::new(id.get())
    }
}

impl From<RoleId> for SerenityRoleId {
    fn from(id: RoleId) -> Self {
        Self::new(id.get())
    }
}

impl RoleSource for SerenityRoleSource<'_> {
    async fn role_catalog(&self, guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        let guild_id = SerenityGuildId::from(*guild_id);
        let roles = guild_id.roles(self.http).await.map_err(map_role_catalog_error)?;
        let bot_member = guild_id
            .member(self.http, self.bot_user_id)
            .await
            .map_err(map_role_catalog_error)?;

        let everyone_id = SerenityRoleId::new(guild_id.get());
        let everyone_permissions = roles
            .get(&everyone_id)
            .map(|role| role.permissions)
            .ok_or_else(|| ManagementError::RoleSource("@everyone Role が存在しません".to_owned()))?;
        let mut bot_permissions = everyone_permissions;
        for role in bot_member.roles.iter().filter_map(|role_id| roles.get(role_id)) {
            bot_permissions |= role.permissions;
        }
        let bot_highest_role = bot_member
            .roles
            .iter()
            .filter_map(|role_id| roles.get(role_id))
            .max()
            .or_else(|| roles.get(&everyone_id))
            .cloned()
            .ok_or_else(|| ManagementError::RoleSource("@everyone Role が存在しません".to_owned()))?;

        if !bot_permissions.intersects(Permissions::MANAGE_ROLES | Permissions::ADMINISTRATOR) {
            return Err(ManagementError::RoleSource(
                "Bot に MANAGE_ROLES 権限がありません".to_owned(),
            ));
        }
        let grantable_permissions = if bot_permissions.contains(Permissions::ADMINISTRATOR) {
            Permissions::all()
        } else {
            bot_permissions
        };
        let known_permissions = permission_vocabulary().known_permissions().collect::<Vec<_>>();

        let mut snapshots = roles
            .into_iter()
            .map(|role| RoleSnapshot {
                id: RoleId::from(role.id),
                // Serenity の Role::Ord が Discord の階層順（position、同値時は Snowflake）を表す。
                // @everyone は通常の階層編集ではなく、基底権限の更新対象として明示的に許可する。
                manageable: role.id == everyone_id || (!role.managed() && role.cmp(&bot_highest_role).is_lt()),
                name: role.name.to_string(),
                color: Color::new(role.colour.0).expect("Discord Role の color は常に24-bit範囲です"),
                hoist: role.hoist(),
                mentionable: role.mentionable(),
                permissions: known_permissions
                    .iter()
                    .cloned()
                    .map(|permission| {
                        let serenity_permission = serenity_permission(&permission);
                        (permission, role.permissions.contains(serenity_permission))
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|role| role.id);
        Ok(RoleCatalog {
            roles: snapshots,
            permission_names: known_permissions.iter().cloned().collect(),
            grantable_permissions: known_permissions
                .iter()
                .filter(|permission| grantable_permissions.contains(serenity_permission(permission)))
                .cloned()
                .collect(),
            default_permissions: known_permissions
                .iter()
                .cloned()
                .map(|permission| {
                    let serenity_permission = serenity_permission(&permission);
                    (permission, everyone_permissions.contains(serenity_permission))
                })
                .collect(),
        })
    }
}

fn map_role_catalog_error(error: SerenityError) -> ManagementError {
    match error {
        SerenityError::Http(error) if error.status_code() == Some(StatusCode::FORBIDDEN) => {
            ManagementError::RoleCatalogPermissionDenied(error.to_string())
        }
        error => ManagementError::RoleSource(error.to_string()),
    }
}

impl RoleUpdater for SerenityRoleSource<'_> {
    async fn update_role(
        &self,
        guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        let mut edit = EditRole::new();
        if let Some(name) = update.name {
            edit = edit.name(name);
        }
        if let Some(color) = update.color {
            edit = edit.colour(Colour::new(color.get()));
        }
        if let Some(hoist) = update.hoist {
            edit = edit.hoist(hoist);
        }
        if let Some(mentionable) = update.mentionable {
            edit = edit.mentionable(mentionable);
        }
        if let Some(permission_values) = update.permissions {
            let mut permissions = Permissions::empty();
            for (name, enabled) in permission_values {
                let permission = serenity_permission(&name);
                if enabled {
                    permissions |= permission;
                }
            }
            edit = edit.permissions(permissions);
        }

        match SerenityGuildId::from(*guild_id)
            .edit_role(self.http, SerenityRoleId::from(*role_id), edit)
            .await
        {
            Ok(_) => Ok(RoleUpdateOutcome::Applied),
            Err(SerenityError::Io(_)) | Err(SerenityError::Http(HttpError::Request(_))) => {
                Ok(RoleUpdateOutcome::ResponseUnknown)
            }
            Err(error) => Err(ManagementError::RoleSource(error.to_string())),
        }
    }
}

impl RoleLifecycleTarget for SerenityRoleSource<'_> {
    async fn create_role(&self, guild_id: &GuildId, create: RoleCreate) -> Result<RoleCreateOutcome, ManagementError> {
        let mut edit = EditRole::new()
            .name(create.name)
            .colour(Colour::new(create.color.get()))
            .hoist(create.hoist)
            .mentionable(create.mentionable);
        let mut permissions = Permissions::empty();
        for (name, enabled) in create.permissions {
            let permission = serenity_permission(&name);
            if enabled {
                permissions |= permission;
            }
        }
        edit = edit.permissions(permissions);

        match SerenityGuildId::from(*guild_id).create_role(self.http, edit).await {
            Ok(role) => Ok(RoleCreateOutcome::Created(RoleId::from(role.id))),
            Err(SerenityError::Io(_)) | Err(SerenityError::Http(HttpError::Request(_))) => {
                Ok(RoleCreateOutcome::ResponseUnknown)
            }
            Err(SerenityError::Http(error)) if error.status_code() == Some(StatusCode::FORBIDDEN) => {
                Err(ManagementError::RolePermissionDenied(error.to_string()))
            }
            Err(error) => Err(ManagementError::RoleSource(error.to_string())),
        }
    }

    async fn delete_role(&self, guild_id: &GuildId, role_id: &RoleId) -> Result<RoleDeleteOutcome, ManagementError> {
        match SerenityGuildId::from(*guild_id)
            .delete_role(self.http, SerenityRoleId::from(*role_id), None)
            .await
        {
            Ok(()) => Ok(RoleDeleteOutcome::Deleted),
            Err(SerenityError::Io(_)) | Err(SerenityError::Http(HttpError::Request(_))) => {
                Ok(RoleDeleteOutcome::ResponseUnknown)
            }
            Err(SerenityError::Http(error)) if error.status_code() == Some(StatusCode::FORBIDDEN) => {
                Err(ManagementError::RolePermissionDenied(error.to_string()))
            }
            Err(error) => Err(ManagementError::RoleSource(error.to_string())),
        }
    }
}
