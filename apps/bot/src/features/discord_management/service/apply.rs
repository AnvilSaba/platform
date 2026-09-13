use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use super::model::{
    DefinitionFile, RoleAttributes, StateFile, compose_attributes, deserialize_state_for_guild, resolve,
    resolve_role_id,
};
use super::{
    ManagementError, RoleApplyOptions, RoleApplyResult, RoleApplyStatus, RoleCreate, RoleCreateOutcome,
    RoleDeleteOutcome, RoleLifecycleChange, RoleLifecycleTarget, RoleManagementService, RolePlan, RoleSnapshot,
    RoleUpdate, RoleUpdateOutcome, build_plan,
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
    S: RoleLifecycleTarget,
{
    pub async fn apply_roles(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        processing_deadline: Instant,
    ) -> Result<RoleApplyResult, ManagementError> {
        self.apply_roles_with_options(
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            RoleApplyOptions::default(),
            processing_deadline,
        )
        .await
    }

    pub async fn apply_roles_with_options(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        options: RoleApplyOptions,
        processing_deadline: Instant,
    ) -> Result<RoleApplyResult, ManagementError> {
        let state = deserialize_state_for_guild(state_json, guild_id)?;
        let mut state = state;
        let Some(guard) = GuildApplyGuard::acquire(guild_id) else {
            return result(
                &state,
                RoleApplyStatus::GuildBusy,
                Vec::new(),
                confirmed_plan.changes.clone(),
                Vec::new(),
                confirmed_plan.lifecycle.clone(),
            );
        };

        if Instant::now() >= processing_deadline {
            return result(
                &state,
                RoleApplyStatus::DeadlineExceeded,
                Vec::new(),
                confirmed_plan.changes.clone(),
                Vec::new(),
                confirmed_plan.lifecycle.clone(),
            );
        }

        let definition: DefinitionFile =
            toml::from_str(definition_toml).map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        let mut applied = Vec::new();
        let mut pending = confirmed_plan.changes.clone();
        let mut applied_lifecycle = Vec::new();
        let mut pending_lifecycle = confirmed_plan.lifecycle.clone();
        let mut catalog = match tokio::time::timeout(
            processing_deadline.saturating_duration_since(Instant::now()),
            self.source.role_catalog(&guild_id),
        )
        .await
        {
            Ok(Ok(catalog)) => catalog,
            Ok(Err(error)) => {
                return result(
                    &state,
                    RoleApplyStatus::Failed(error.to_string()),
                    applied,
                    pending,
                    applied_lifecycle,
                    pending_lifecycle,
                );
            }
            Err(_) => {
                return result(
                    &state,
                    RoleApplyStatus::DeadlineExceeded,
                    applied,
                    pending,
                    applied_lifecycle,
                    pending_lifecycle,
                );
            }
        };
        let current_plan = build_plan(&definition, &state, &catalog)?;
        if current_plan != *confirmed_plan {
            return result(
                &state,
                RoleApplyStatus::ReplanRequired,
                Vec::new(),
                current_plan.changes,
                Vec::new(),
                current_plan.lifecycle,
            );
        }
        if !options.allow_deletions
            && current_plan
                .lifecycle
                .iter()
                .any(|change| matches!(change, RoleLifecycleChange::Delete { .. }))
        {
            return result(
                &state,
                RoleApplyStatus::DeletionPermissionRequired,
                Vec::new(),
                current_plan.changes,
                Vec::new(),
                current_plan.lifecycle,
            );
        }

        for (logical_id, role_definition) in &definition.roles {
            let lifecycle = pending_lifecycle
                .iter()
                .find(|change| lifecycle_logical_id(change) == logical_id)
                .cloned();

            if let Some(lifecycle) = lifecycle {
                match lifecycle.clone() {
                    RoleLifecycleChange::Create { .. } => {
                        if Instant::now() >= processing_deadline {
                            return result(
                                &state,
                                RoleApplyStatus::DeadlineExceeded,
                                applied,
                                pending,
                                applied_lifecycle,
                                pending_lifecycle,
                            );
                        }
                        let desired = compose_attributes(role_definition, &definition.settings_sets.role);
                        let create = build_role_create(
                            &desired,
                            &catalog.permission_names,
                            &catalog.default_permissions,
                            logical_id,
                        )?;
                        state.pending_creations.insert(logical_id.clone());
                        let outcome = match tokio::time::timeout(
                            processing_deadline.saturating_duration_since(Instant::now()),
                            self.source.create_role(&guild_id, create),
                        )
                        .await
                        {
                            Ok(Ok(outcome)) => outcome,
                            Ok(Err(error)) => {
                                state.pending_creations.remove(logical_id);
                                return result(
                                    &state,
                                    RoleApplyStatus::Failed(error.to_string()),
                                    applied,
                                    pending,
                                    applied_lifecycle,
                                    pending_lifecycle,
                                );
                            }
                            Err(_) => RoleCreateOutcome::ResponseUnknown,
                        };
                        let RoleCreateOutcome::Created(role_id) = outcome else {
                            return result(
                                &state,
                                RoleApplyStatus::CreationResponseUnknown,
                                applied,
                                pending,
                                applied_lifecycle,
                                pending_lifecycle,
                            );
                        };
                        if state.roles.values().any(|existing_id| *existing_id == role_id) {
                            state.pending_creations.remove(logical_id);
                            return Err(ManagementError::InvalidState(format!(
                                "新しく作成した Role {role_id} は既存の対応と衝突しています"
                            )));
                        }
                        state.roles.insert(logical_id.clone(), role_id);
                        state.deleted_roles.remove(logical_id);
                        state.pending_creations.remove(logical_id);
                        pending_lifecycle.retain(|change| change != &lifecycle);
                        applied_lifecycle.push(lifecycle);

                        let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                        catalog = match tokio::time::timeout(
                            refresh_deadline.saturating_duration_since(Instant::now()),
                            self.source.role_catalog(&guild_id),
                        )
                        .await
                        {
                            Ok(Ok(catalog)) => catalog,
                            Ok(Err(error)) => {
                                return result(
                                    &state,
                                    RoleApplyStatus::Failed(error.to_string()),
                                    applied,
                                    pending,
                                    applied_lifecycle,
                                    pending_lifecycle,
                                );
                            }
                            Err(_) => {
                                return result(
                                    &state,
                                    RoleApplyStatus::DeadlineExceeded,
                                    applied,
                                    pending,
                                    applied_lifecycle,
                                    pending_lifecycle,
                                );
                            }
                        };
                    }
                    RoleLifecycleChange::Delete { discord_id, .. } => {
                        if !options.allow_deletions {
                            return result(
                                &state,
                                RoleApplyStatus::DeletionPermissionRequired,
                                applied,
                                pending,
                                applied_lifecycle,
                                pending_lifecycle,
                            );
                        }
                        if Instant::now() >= processing_deadline {
                            return result(
                                &state,
                                RoleApplyStatus::DeadlineExceeded,
                                applied,
                                pending,
                                applied_lifecycle,
                                pending_lifecycle,
                            );
                        }
                        state.pending_deletions.insert(logical_id.clone());
                        let exists = catalog.roles.iter().any(|role| role.id == discord_id);
                        if !exists {
                            state.pending_deletions.remove(logical_id);
                            state.deleted_roles.insert(logical_id.clone());
                            pending_lifecycle.retain(|change| change != &lifecycle);
                            applied_lifecycle.push(lifecycle);
                            continue;
                        }
                        let outcome = match tokio::time::timeout(
                            processing_deadline.saturating_duration_since(Instant::now()),
                            self.source.delete_role(&guild_id, &discord_id),
                        )
                        .await
                        {
                            Ok(Ok(outcome)) => outcome,
                            Ok(Err(error)) => {
                                let status = match error {
                                    ManagementError::RolePermissionDenied(message) => {
                                        RoleApplyStatus::DeletionPermissionDenied(message)
                                    }
                                    error => RoleApplyStatus::Failed(error.to_string()),
                                };
                                return result(&state, status, applied, pending, applied_lifecycle, pending_lifecycle);
                            }
                            Err(_) => RoleDeleteOutcome::ResponseUnknown,
                        };
                        if outcome == RoleDeleteOutcome::ResponseUnknown {
                            let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                            catalog = match tokio::time::timeout(
                                refresh_deadline.saturating_duration_since(Instant::now()),
                                self.source.role_catalog(&guild_id),
                            )
                            .await
                            {
                                Ok(Ok(catalog)) => catalog,
                                Ok(Err(error)) => {
                                    let status = match error {
                                        ManagementError::RoleCatalogPermissionDenied(message) => {
                                            RoleApplyStatus::DeletionVerificationPermissionDenied(message)
                                        }
                                        error => RoleApplyStatus::DeletionVerificationIndeterminate(error.to_string()),
                                    };
                                    return result(
                                        &state,
                                        status,
                                        applied,
                                        pending,
                                        applied_lifecycle,
                                        pending_lifecycle,
                                    );
                                }
                                Err(_) => {
                                    return result(
                                        &state,
                                        RoleApplyStatus::DeletionVerificationIndeterminate(
                                            "削除後の Role 存在確認が期限内に完了しませんでした".to_owned(),
                                        ),
                                        applied,
                                        pending,
                                        applied_lifecycle,
                                        pending_lifecycle,
                                    );
                                }
                            };
                            if catalog.roles.iter().any(|role| role.id == discord_id) {
                                return result(
                                    &state,
                                    RoleApplyStatus::DeletionResponseUnknown,
                                    applied,
                                    pending,
                                    applied_lifecycle,
                                    pending_lifecycle,
                                );
                            }
                        }
                        state.pending_deletions.remove(logical_id);
                        state.deleted_roles.insert(logical_id.clone());
                        pending_lifecycle.retain(|change| change != &lifecycle);
                        applied_lifecycle.push(lifecycle);
                        if outcome == RoleDeleteOutcome::Deleted {
                            let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                            catalog = match tokio::time::timeout(
                                refresh_deadline.saturating_duration_since(Instant::now()),
                                self.source.role_catalog(&guild_id),
                            )
                            .await
                            {
                                Ok(Ok(catalog)) => catalog,
                                Ok(Err(error)) => {
                                    return result(
                                        &state,
                                        RoleApplyStatus::Failed(error.to_string()),
                                        applied,
                                        pending,
                                        applied_lifecycle,
                                        pending_lifecycle,
                                    );
                                }
                                Err(_) => {
                                    return result(
                                        &state,
                                        RoleApplyStatus::DeadlineExceeded,
                                        applied,
                                        pending,
                                        applied_lifecycle,
                                        pending_lifecycle,
                                    );
                                }
                            };
                        }
                    }
                    RoleLifecycleChange::Release { .. } => {}
                }
            }

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
                return result(
                    &state,
                    RoleApplyStatus::DeadlineExceeded,
                    applied,
                    pending,
                    applied_lifecycle,
                    pending_lifecycle,
                );
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
                    return result(
                        &state,
                        RoleApplyStatus::Failed(error.to_string()),
                        applied,
                        pending,
                        applied_lifecycle,
                        pending_lifecycle,
                    );
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
                    return result(
                        &state,
                        RoleApplyStatus::Failed(error.to_string()),
                        applied,
                        pending,
                        applied_lifecycle,
                        pending_lifecycle,
                    );
                }
                Err(_) => {
                    return result(
                        &state,
                        if outcome == RoleUpdateOutcome::Applied {
                            RoleApplyStatus::DeadlineExceeded
                        } else {
                            RoleApplyStatus::ResponseUnknown
                        },
                        applied,
                        pending,
                        applied_lifecycle,
                        pending_lifecycle,
                    );
                }
            };
            let matches = catalog
                .roles
                .iter()
                .find(|role| role.id == role_id)
                .is_some_and(|role| role_matches_update(role, &update));
            if !matches {
                return result(
                    &state,
                    if outcome == RoleUpdateOutcome::ResponseUnknown {
                        RoleApplyStatus::ResponseUnknown
                    } else {
                        RoleApplyStatus::Failed(format!("Role {logical_id} の更新後の値が希望値と一致しません"))
                    },
                    applied,
                    pending,
                    applied_lifecycle,
                    pending_lifecycle,
                );
            }

            if outcome == RoleUpdateOutcome::ResponseUnknown {
                applied.extend(role_changes);
                pending.retain(|change| &change.logical_id != logical_id);
            }
        }

        for lifecycle in confirmed_plan
            .lifecycle
            .iter()
            .filter(|change| matches!(change, RoleLifecycleChange::Release { .. }))
        {
            if let RoleLifecycleChange::Release { logical_id, .. } = lifecycle {
                state.roles.remove(logical_id);
                state.deleted_roles.remove(logical_id);
                state.pending_deletions.remove(logical_id);
                state.pending_creations.remove(logical_id);
                pending_lifecycle.retain(|change| change != lifecycle);
                applied_lifecycle.push(lifecycle.clone());
            }
        }

        drop(guard);
        result(
            &state,
            RoleApplyStatus::Complete,
            applied,
            pending,
            applied_lifecycle,
            pending_lifecycle,
        )
    }
}

fn result(
    state: &StateFile,
    status: RoleApplyStatus,
    applied: Vec<super::AttributeChange>,
    pending: Vec<super::AttributeChange>,
    applied_lifecycle: Vec<RoleLifecycleChange>,
    pending_lifecycle: Vec<RoleLifecycleChange>,
) -> Result<RoleApplyResult, ManagementError> {
    Ok(RoleApplyResult {
        status,
        applied,
        pending,
        applied_lifecycle,
        pending_lifecycle,
        state_json: serialize_state(state)?,
    })
}

fn lifecycle_logical_id(change: &RoleLifecycleChange) -> &RoleLogicalId {
    match change {
        RoleLifecycleChange::Create { logical_id, .. }
        | RoleLifecycleChange::Release { logical_id, .. }
        | RoleLifecycleChange::Delete { logical_id, .. } => logical_id,
    }
}

fn serialize_state(state: &StateFile) -> Result<String, ManagementError> {
    serde_json::to_string_pretty(state)
        .map(|json| format!("{json}\n"))
        .map_err(|error| ManagementError::SerializeState(error.to_string()))
}

fn build_role_create(
    desired: &RoleAttributes,
    permission_names: &BTreeSet<String>,
    default_permissions: &BTreeMap<String, bool>,
    logical_id: &RoleLogicalId,
) -> Result<RoleCreate, ManagementError> {
    let name = desired
        .name
        .as_ref()
        .ok_or_else(|| ManagementError::InvalidDefinition(format!("新しい Role {logical_id} には name が必要です")))?;
    let name = resolve(name, "new role".to_owned(), "name")?;
    let color = desired
        .color
        .as_ref()
        .map(|value| resolve(value, 0, "color"))
        .transpose()?
        .unwrap_or(0);
    let hoist = desired
        .hoist
        .as_ref()
        .map(|value| resolve(value, false, "hoist"))
        .transpose()?
        .unwrap_or(false);
    let mentionable = desired
        .mentionable
        .as_ref()
        .map(|value| resolve(value, false, "mentionable"))
        .transpose()?
        .unwrap_or(false);
    let mut permissions = permission_names
        .iter()
        .map(|permission| (permission.clone(), false))
        .collect::<BTreeMap<_, _>>();
    for (permission, value) in &desired.permissions {
        let default = *default_permissions
            .get(permission)
            .ok_or_else(|| ManagementError::RoleSource(format!("権限 {permission} の Guild 既定値を取得できません")))?;
        permissions.insert(
            permission.clone(),
            resolve(value, default, &format!("permissions.{permission}"))?,
        );
    }
    Ok(RoleCreate {
        name,
        color,
        hoist,
        mentionable,
        permissions,
    })
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
