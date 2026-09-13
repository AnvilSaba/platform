use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use super::{
    ManagementError, RoleApplyResult, RoleApplyStatus, RoleManagementService, RolePlan, RoleSnapshot, RoleTarget,
    RoleUpdate, RoleUpdateOutcome, build_plan,
};
use super::model::{
    DefinitionFile, RoleAttributes, StateFile, compose_attributes, deserialize_state_for_guild, resolve,
    resolve_role_id,
};
use crate::features::discord_management::ids::{GuildId, RoleLogicalId};

const RESULT_STATE_REFRESH_BUDGET: Duration = Duration::from_secs(90);

static APPLYING_GUILDS: OnceLock<Mutex<BTreeSet<GuildId>>> = OnceLock::new();

struct GuildApplyGuard(GuildId);

impl GuildApplyGuard {
    fn acquire(guild_id: GuildId) -> Option<Self> {
        let mut guilds = APPLYING_GUILDS
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
            .expect("guild apply mutex poisoned");
        if guilds.insert(guild_id) {
            Some(Self(guild_id))
        } else {
            None
        }
    }
}

impl Drop for GuildApplyGuard {
    fn drop(&mut self) {
        APPLYING_GUILDS
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
            .expect("guild apply mutex poisoned")
            .remove(&self.0);
    }
}

impl<S> RoleManagementService<S>
where
    S: RoleTarget,
{
    pub async fn apply_roles(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        processing_deadline: Instant,
    ) -> Result<RoleApplyResult, ManagementError> {
        let state = deserialize_state_for_guild(state_json, guild_id)?;
        let latest_state_json = serialize_state(&state)?;
        let Some(guard) = GuildApplyGuard::acquire(guild_id) else {
            return Ok(RoleApplyResult {
                status: RoleApplyStatus::GuildBusy,
                applied: Vec::new(),
                pending: confirmed_plan.changes.clone(),
                state_json: latest_state_json,
            });
        };

        if Instant::now() >= processing_deadline {
            return Ok(RoleApplyResult {
                status: RoleApplyStatus::DeadlineExceeded,
                applied: Vec::new(),
                pending: confirmed_plan.changes.clone(),
                state_json: latest_state_json,
            });
        }

        let definition: DefinitionFile =
            toml::from_str(definition_toml).map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        let mut applied = Vec::new();
        let mut pending = confirmed_plan.changes.clone();
        let mut catalog = match tokio::time::timeout(
            processing_deadline.saturating_duration_since(Instant::now()),
            self.source.role_catalog(&guild_id),
        )
        .await
        {
            Ok(Ok(catalog)) => catalog,
            Ok(Err(error)) => {
                return Ok(RoleApplyResult {
                    status: RoleApplyStatus::Failed(error.to_string()),
                    applied,
                    pending,
                    state_json: latest_state_json,
                });
            }
            Err(_) => {
                return Ok(RoleApplyResult {
                    status: RoleApplyStatus::DeadlineExceeded,
                    applied,
                    pending,
                    state_json: latest_state_json,
                });
            }
        };
        let current_plan = build_plan(&definition, &state, &catalog)?;
        if current_plan != *confirmed_plan {
            return Ok(RoleApplyResult {
                status: RoleApplyStatus::ReplanRequired,
                applied: Vec::new(),
                pending: current_plan.changes,
                state_json: latest_state_json,
            });
        }

        for (logical_id, role_definition) in &definition.roles {
            let role_id = resolve_role_id(logical_id, &state)?;
            let role_changes = pending
                .iter()
                .filter(|change| &change.logical_id == logical_id)
                .cloned()
                .collect::<Vec<_>>();
            if role_changes.is_empty() {
                continue;
            }
            if Instant::now() >= processing_deadline {
                return Ok(RoleApplyResult {
                    status: RoleApplyStatus::DeadlineExceeded,
                    applied,
                    pending,
                    state_json: latest_state_json,
                });
            }

            let actual = catalog.roles.iter().find(|role| role.id == role_id).ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {role_id} が Guild に存在しません"
                ))
            })?;
            let desired = compose_attributes(role_definition, &definition.settings_sets.role);
            let update = build_role_update(actual, &desired, &catalog.default_permissions, logical_id)?;
            if update.is_empty() {
                continue;
            }

            let outcome = match tokio::time::timeout(
                processing_deadline.saturating_duration_since(Instant::now()),
                self.source.update_role(&guild_id, &role_id, update.clone()),
            )
            .await
            {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(error)) => {
                    return Ok(RoleApplyResult {
                        status: RoleApplyStatus::Failed(error.to_string()),
                        applied,
                        pending,
                        state_json: latest_state_json,
                    });
                }
                Err(_) => RoleUpdateOutcome::ResponseUnknown,
            };
            if outcome == RoleUpdateOutcome::Applied {
                applied.extend(role_changes.clone());
                pending.retain(|change| &change.logical_id != logical_id);
            }

            let result_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
            catalog = match tokio::time::timeout(
                result_deadline.saturating_duration_since(Instant::now()),
                self.source.role_catalog(&guild_id),
            )
            .await
            {
                Ok(Ok(catalog)) => catalog,
                Ok(Err(error)) => {
                    return Ok(RoleApplyResult {
                        status: RoleApplyStatus::Failed(error.to_string()),
                        applied,
                        pending,
                        state_json: latest_state_json,
                    });
                }
                Err(_) => {
                    return Ok(RoleApplyResult {
                        status: if outcome == RoleUpdateOutcome::Applied {
                            RoleApplyStatus::DeadlineExceeded
                        } else {
                            RoleApplyStatus::ResponseUnknown
                        },
                        applied,
                        pending,
                        state_json: latest_state_json,
                    });
                }
            };
            let matches = catalog
                .roles
                .iter()
                .find(|role| role.id == role_id)
                .is_some_and(|role| role_matches_update(role, &update));
            if !matches {
                return Ok(RoleApplyResult {
                    status: if outcome == RoleUpdateOutcome::ResponseUnknown {
                        RoleApplyStatus::ResponseUnknown
                    } else {
                        RoleApplyStatus::Failed(format!("Role {logical_id} の更新後の値が希望値と一致しません"))
                    },
                    applied,
                    pending,
                    state_json: latest_state_json,
                });
            }

            if outcome == RoleUpdateOutcome::ResponseUnknown {
                applied.extend(role_changes);
                pending.retain(|change| &change.logical_id != logical_id);
            }
        }

        drop(guard);
        Ok(RoleApplyResult {
            status: RoleApplyStatus::Complete,
            applied,
            pending,
            state_json: latest_state_json,
        })
    }
}

fn serialize_state(state: &StateFile) -> Result<String, ManagementError> {
    serde_json::to_string_pretty(state)
        .map(|json| format!("{json}\n"))
        .map_err(|error| ManagementError::SerializeState(error.to_string()))
}

fn build_role_update(
    actual: &RoleSnapshot,
    desired: &RoleAttributes,
    default_permissions: &BTreeMap<String, bool>,
    logical_id: &RoleLogicalId,
) -> Result<RoleUpdate, ManagementError> {
    let mut update = RoleUpdate::default();
    if let Some(value) = &desired.name {
        let value = resolve(value, "new role".to_owned(), "name")?;
        if value != actual.name {
            update.name = Some(value);
        }
    }
    if let Some(value) = &desired.color {
        let value = resolve(value, 0, "color")?;
        if value != actual.color {
            update.color = Some(value);
        }
    }
    if let Some(value) = &desired.hoist {
        let value = resolve(value, false, "hoist")?;
        if value != actual.hoist {
            update.hoist = Some(value);
        }
    }
    if let Some(value) = &desired.mentionable {
        let value = resolve(value, false, "mentionable")?;
        if value != actual.mentionable {
            update.mentionable = Some(value);
        }
    }
    if !desired.permissions.is_empty() {
        let mut permissions = actual.permissions.clone();
        for (permission, value) in &desired.permissions {
            let current = actual.permissions.get(permission).ok_or_else(|| {
                ManagementError::InvalidDefinition(format!(
                    "Role {logical_id} に未知の権限 {permission} が指定されています"
                ))
            })?;
            let default = *default_permissions.get(permission).ok_or_else(|| {
                ManagementError::RoleSource(format!("権限 {permission} の Guild 既定値を取得できません"))
            })?;
            let value = resolve(value, default, &format!("permissions.{permission}"))?;
            if value != *current {
                permissions.insert(permission.clone(), value);
            }
        }
        if permissions != actual.permissions {
            update.permissions = Some(permissions);
        }
    }
    Ok(update)
}

fn role_matches_update(role: &RoleSnapshot, update: &RoleUpdate) -> bool {
    update.name.as_ref().is_none_or(|name| role.name == *name)
        && update.color.is_none_or(|color| role.color == color)
        && update.hoist.is_none_or(|hoist| role.hoist == hoist)
        && update
            .mentionable
            .is_none_or(|mentionable| role.mentionable == mentionable)
        && update
            .permissions
            .as_ref()
            .is_none_or(|permissions| role.permissions == *permissions)
}
