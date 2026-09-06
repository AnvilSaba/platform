use std::collections::BTreeMap;

use serenity::all::{GuildId as SerenityGuildId, Http, Permissions, RoleId as SerenityRoleId, UserId};

use super::ids::{GuildId, RoleId};
use super::service::{ManagementError, RoleCatalog, RoleSnapshot, RoleSource};

impl From<SerenityGuildId> for GuildId {
    fn from(id: SerenityGuildId) -> Self {
        Self::new(id.get())
    }
}

impl From<GuildId> for SerenityGuildId {
    fn from(id: GuildId) -> Self {
        Self::new(id.get())
    }
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

fn is_lower_in_hierarchy(position: i16, id: SerenityRoleId, highest_position: i16, highest_id: SerenityRoleId) -> bool {
    position < highest_position || (position == highest_position && id > highest_id)
}

pub struct SerenityRoleSource<'a> {
    http: &'a Http,
    bot_user_id: UserId,
}

impl<'a> SerenityRoleSource<'a> {
    pub fn new(http: &'a Http, bot_user_id: UserId) -> Self {
        Self { http, bot_user_id }
    }
}

impl RoleSource for SerenityRoleSource<'_> {
    async fn role_catalog(&self, guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        let guild_id = SerenityGuildId::from(*guild_id);
        let roles = guild_id
            .roles(self.http)
            .await
            .map_err(|error| ManagementError::RoleSource(error.to_string()))?;
        let bot_member = guild_id
            .member(self.http, self.bot_user_id)
            .await
            .map_err(|error| ManagementError::RoleSource(error.to_string()))?;

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

        let mut snapshots = roles
            .into_iter()
            .map(|role| RoleSnapshot {
                id: RoleId::from(role.id),
                manageable: role.id != everyone_id
                    && !role.managed()
                    && is_lower_in_hierarchy(role.position, role.id, bot_highest_role.position, bot_highest_role.id),
                name: role.name.to_string(),
                color: role.colour.0,
                hoist: role.hoist(),
                mentionable: role.mentionable(),
                permissions: Permissions::all()
                    .iter_names()
                    .map(|(name, permission)| (name.to_owned(), role.permissions.contains(permission)))
                    .collect::<BTreeMap<_, _>>(),
            })
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|role| role.id);
        Ok(RoleCatalog {
            roles: snapshots,
            permission_names: Permissions::all()
                .iter_names()
                .map(|(name, _)| name.to_owned())
                .collect(),
            default_permissions: Permissions::all()
                .iter_names()
                .map(|(name, permission)| (name.to_owned(), everyone_permissions.contains(permission)))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_position_role_with_larger_snowflake_is_lower() {
        assert!(is_lower_in_hierarchy(
            10,
            SerenityRoleId::new(201),
            10,
            SerenityRoleId::new(200)
        ));
        assert!(!is_lower_in_hierarchy(
            10,
            SerenityRoleId::new(199),
            10,
            SerenityRoleId::new(200)
        ));
    }
}
