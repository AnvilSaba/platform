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
    ManagementError, RoleApplyOptions, RoleApplyResult, RoleApplyStatus, RoleCatalog, RoleCreate, RoleCreateOutcome,
    RoleDeleteOutcome, RoleLifecycleChange, RoleLifecycleTarget, RoleManagementService, RolePlan, RoleSnapshot,
    RoleUpdate, RoleUpdateOutcome, RoleUpdater, build_plan,
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

struct ApplySession {
    _guard: GuildApplyGuard,
    state: StateFile,
    definition: DefinitionFile,
    catalog: RoleCatalog,
    applied: Vec<super::AttributeChange>,
    pending: Vec<super::AttributeChange>,
    applied_lifecycle: Vec<RoleLifecycleChange>,
    pending_lifecycle: Vec<RoleLifecycleChange>,
}

enum ApplyPreparation {
    Finished(RoleApplyResult),
    Ready(Box<ApplySession>),
}

impl ApplySession {
    fn into_result(self, status: RoleApplyStatus) -> Result<RoleApplyResult, ManagementError> {
        let Self {
            state,
            applied,
            pending,
            applied_lifecycle,
            pending_lifecycle,
            ..
        } = self;
        result(&state, status, applied, pending, applied_lifecycle, pending_lifecycle)
    }
}

impl<S> RoleManagementService<S>
where
    S: RoleUpdater,
{
    pub async fn apply_role_updates(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        processing_deadline: Instant,
    ) -> Result<RoleApplyResult, ManagementError> {
        let preparation = self
            .prepare_apply(
                guild_id,
                definition_toml,
                state_json,
                confirmed_plan,
                processing_deadline,
            )
            .await?;
        let mut session = match preparation {
            ApplyPreparation::Finished(result) => return Ok(result),
            ApplyPreparation::Ready(session) => *session,
        };
        if !session.pending_lifecycle.is_empty() {
            return Err(ManagementError::InvalidState(
                "属性更新専用の apply_role_updates には lifecycle 変更を含められません".to_owned(),
            ));
        }

        let desired_attributes = desired_role_attributes(&session.definition);
        for (logical_id, desired) in desired_attributes {
            if let Some(status) = self
                .apply_attribute_changes(&guild_id, &logical_id, &desired, &mut session, processing_deadline)
                .await?
            {
                return session.into_result(status);
            }
        }

        session.into_result(RoleApplyStatus::Complete)
    }

    async fn prepare_apply(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        processing_deadline: Instant,
    ) -> Result<ApplyPreparation, ManagementError> {
        let state = deserialize_state_for_guild(state_json, guild_id)?;
        let Some(guard) = GuildApplyGuard::acquire(guild_id) else {
            return Ok(ApplyPreparation::Finished(result(
                &state,
                RoleApplyStatus::GuildBusy,
                Vec::new(),
                confirmed_plan.changes.clone(),
                Vec::new(),
                confirmed_plan.lifecycle.clone(),
            )?));
        };

        if Instant::now() >= processing_deadline {
            return Ok(ApplyPreparation::Finished(result(
                &state,
                RoleApplyStatus::DeadlineExceeded,
                Vec::new(),
                confirmed_plan.changes.clone(),
                Vec::new(),
                confirmed_plan.lifecycle.clone(),
            )?));
        }

        let definition: DefinitionFile =
            toml::from_str(definition_toml).map_err(|error| ManagementError::InvalidDefinition(error.to_string()))?;
        let applied = Vec::new();
        let pending = confirmed_plan.changes.clone();
        let applied_lifecycle = Vec::new();
        let pending_lifecycle = confirmed_plan.lifecycle.clone();
        let catalog = match tokio::time::timeout(
            processing_deadline.saturating_duration_since(Instant::now()),
            self.source.role_catalog(&guild_id),
        )
        .await
        {
            Ok(Ok(catalog)) => catalog,
            Ok(Err(error)) => {
                return Ok(ApplyPreparation::Finished(result(
                    &state,
                    RoleApplyStatus::Failed(error.to_string()),
                    applied,
                    pending,
                    applied_lifecycle,
                    pending_lifecycle,
                )?));
            }
            Err(_) => {
                return Ok(ApplyPreparation::Finished(result(
                    &state,
                    RoleApplyStatus::DeadlineExceeded,
                    applied,
                    pending,
                    applied_lifecycle,
                    pending_lifecycle,
                )?));
            }
        };
        let current_plan = build_plan(&definition, &state, &catalog)?;
        if current_plan != *confirmed_plan {
            return Ok(ApplyPreparation::Finished(result(
                &state,
                RoleApplyStatus::ReplanRequired,
                Vec::new(),
                current_plan.changes,
                Vec::new(),
                current_plan.lifecycle,
            )?));
        }

        Ok(ApplyPreparation::Ready(Box::new(ApplySession {
            _guard: guard,
            state,
            definition,
            catalog,
            applied,
            pending,
            applied_lifecycle,
            pending_lifecycle,
        })))
    }

    async fn apply_attribute_changes(
        &self,
        guild_id: &GuildId,
        logical_id: &super::RoleLogicalId,
        desired: &RoleAttributes,
        session: &mut ApplySession,
        processing_deadline: Instant,
    ) -> Result<Option<RoleApplyStatus>, ManagementError> {
        let role_id = resolve_role_id(logical_id, &session.state)?;
        let role_changes = session
            .pending
            .iter()
            .filter(|change| &change.logical_id == logical_id)
            .cloned()
            .collect::<Vec<_>>();
        if role_changes.is_empty() {
            return Ok(None);
        }
        if Instant::now() >= processing_deadline {
            return Ok(Some(RoleApplyStatus::DeadlineExceeded));
        }

        let update = {
            let actual = session
                .catalog
                .roles
                .iter()
                .find(|role| role.id == role_id)
                .ok_or_else(|| {
                    ManagementError::InvalidState(format!(
                        "Role {logical_id} の Snowflake {role_id} が Guild に存在しません"
                    ))
                })?;
            build_role_update(actual, desired, &session.catalog.default_permissions, logical_id)?
        };
        if update.is_empty() {
            return Ok(None);
        }

        let outcome = match tokio::time::timeout(
            processing_deadline.saturating_duration_since(Instant::now()),
            self.source.update_role(guild_id, &role_id, update.clone()),
        )
        .await
        {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(error)) => return Ok(Some(RoleApplyStatus::Failed(error.to_string()))),
            Err(_) => RoleUpdateOutcome::ResponseUnknown,
        };
        if outcome == RoleUpdateOutcome::Applied {
            session.applied.extend(role_changes.clone());
            session.pending.retain(|change| &change.logical_id != logical_id);
        }

        let result_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
        session.catalog = match tokio::time::timeout(
            result_deadline.saturating_duration_since(Instant::now()),
            self.source.role_catalog(guild_id),
        )
        .await
        {
            Ok(Ok(catalog)) => catalog,
            Ok(Err(error)) => return Ok(Some(RoleApplyStatus::Failed(error.to_string()))),
            Err(_) => {
                return Ok(Some(if outcome == RoleUpdateOutcome::Applied {
                    RoleApplyStatus::DeadlineExceeded
                } else {
                    RoleApplyStatus::ResponseUnknown
                }));
            }
        };
        let matches = session
            .catalog
            .roles
            .iter()
            .find(|role| role.id == role_id)
            .is_some_and(|role| role_matches_update(role, &update));
        if !matches {
            return Ok(Some(if outcome == RoleUpdateOutcome::ResponseUnknown {
                RoleApplyStatus::ResponseUnknown
            } else {
                RoleApplyStatus::Failed(format!("Role {logical_id} の更新後の値が希望値と一致しません"))
            }));
        }

        if outcome == RoleUpdateOutcome::ResponseUnknown {
            session.applied.extend(role_changes);
            session.pending.retain(|change| &change.logical_id != logical_id);
        }
        Ok(None)
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
        if confirmed_plan.lifecycle.is_empty() {
            return self
                .apply_role_updates(
                    guild_id,
                    definition_toml,
                    state_json,
                    confirmed_plan,
                    processing_deadline,
                )
                .await;
        }

        let preparation = self
            .prepare_apply(
                guild_id,
                definition_toml,
                state_json,
                confirmed_plan,
                processing_deadline,
            )
            .await?;
        let mut session = match preparation {
            ApplyPreparation::Finished(result) => return Ok(result),
            ApplyPreparation::Ready(session) => *session,
        };
        if !options.allow_deletions
            && session
                .pending_lifecycle
                .iter()
                .any(|change| matches!(change, RoleLifecycleChange::Delete { .. }))
        {
            return session.into_result(RoleApplyStatus::DeletionPermissionRequired);
        }

        let desired_attributes = desired_role_attributes(&session.definition);
        for (logical_id, desired) in desired_attributes {
            let lifecycle = session
                .pending_lifecycle
                .iter()
                .find(|change| lifecycle_logical_id(change) == &logical_id)
                .cloned();

            if let Some(lifecycle) = lifecycle {
                match lifecycle.clone() {
                    RoleLifecycleChange::Create { .. } => {
                        if Instant::now() >= processing_deadline {
                            return session.into_result(RoleApplyStatus::DeadlineExceeded);
                        }
                        let create = build_role_create(
                            &desired,
                            &session.catalog.permission_names,
                            &session.catalog.default_permissions,
                            &logical_id,
                        )?;
                        session.state.pending_creations.insert(logical_id.clone());
                        let outcome = match tokio::time::timeout(
                            processing_deadline.saturating_duration_since(Instant::now()),
                            self.source.create_role(&guild_id, create),
                        )
                        .await
                        {
                            Ok(Ok(outcome)) => outcome,
                            Ok(Err(error)) => {
                                session.state.pending_creations.remove(&logical_id);
                                return session.into_result(RoleApplyStatus::Failed(error.to_string()));
                            }
                            Err(_) => RoleCreateOutcome::ResponseUnknown,
                        };
                        let RoleCreateOutcome::Created(role_id) = outcome else {
                            return session.into_result(RoleApplyStatus::CreationResponseUnknown);
                        };
                        if session.state.roles.values().any(|existing_id| *existing_id == role_id) {
                            session.state.pending_creations.remove(&logical_id);
                            return Err(ManagementError::InvalidState(format!(
                                "新しく作成した Role {role_id} は既存の対応と衝突しています"
                            )));
                        }
                        session.state.roles.insert(logical_id.clone(), role_id);
                        session.state.deleted_roles.remove(&logical_id);
                        session.state.pending_creations.remove(&logical_id);
                        session.pending_lifecycle.retain(|change| change != &lifecycle);
                        session.applied_lifecycle.push(lifecycle);

                        let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                        session.catalog = match tokio::time::timeout(
                            refresh_deadline.saturating_duration_since(Instant::now()),
                            self.source.role_catalog(&guild_id),
                        )
                        .await
                        {
                            Ok(Ok(catalog)) => catalog,
                            Ok(Err(error)) => {
                                return session.into_result(RoleApplyStatus::Failed(error.to_string()));
                            }
                            Err(_) => {
                                return session.into_result(RoleApplyStatus::DeadlineExceeded);
                            }
                        };
                    }
                    RoleLifecycleChange::Delete { discord_id, .. } => {
                        if !options.allow_deletions {
                            return session.into_result(RoleApplyStatus::DeletionPermissionRequired);
                        }
                        if Instant::now() >= processing_deadline {
                            return session.into_result(RoleApplyStatus::DeadlineExceeded);
                        }
                        session.state.pending_deletions.insert(logical_id.clone());
                        let exists = session.catalog.roles.iter().any(|role| role.id == discord_id);
                        if !exists {
                            session.state.pending_deletions.remove(&logical_id);
                            session.state.deleted_roles.insert(logical_id.clone());
                            session.pending_lifecycle.retain(|change| change != &lifecycle);
                            session.applied_lifecycle.push(lifecycle);
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
                                return session.into_result(status);
                            }
                            Err(_) => RoleDeleteOutcome::ResponseUnknown,
                        };
                        if outcome == RoleDeleteOutcome::ResponseUnknown {
                            let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                            session.catalog = match tokio::time::timeout(
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
                                    return session.into_result(status);
                                }
                                Err(_) => {
                                    return session.into_result(RoleApplyStatus::DeletionVerificationIndeterminate(
                                        "削除後の Role 存在確認が期限内に完了しませんでした".to_owned(),
                                    ));
                                }
                            };
                            if session.catalog.roles.iter().any(|role| role.id == discord_id) {
                                return session.into_result(RoleApplyStatus::DeletionResponseUnknown);
                            }
                        }
                        session.state.pending_deletions.remove(&logical_id);
                        session.state.deleted_roles.insert(logical_id.clone());
                        session.pending_lifecycle.retain(|change| change != &lifecycle);
                        session.applied_lifecycle.push(lifecycle);
                        if outcome == RoleDeleteOutcome::Deleted {
                            let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                            session.catalog = match tokio::time::timeout(
                                refresh_deadline.saturating_duration_since(Instant::now()),
                                self.source.role_catalog(&guild_id),
                            )
                            .await
                            {
                                Ok(Ok(catalog)) => catalog,
                                Ok(Err(error)) => {
                                    return session.into_result(RoleApplyStatus::Failed(error.to_string()));
                                }
                                Err(_) => {
                                    return session.into_result(RoleApplyStatus::DeadlineExceeded);
                                }
                            };
                        }
                    }
                    RoleLifecycleChange::Release { .. } => {}
                }
            }

            if let Some(status) = self
                .apply_attribute_changes(&guild_id, &logical_id, &desired, &mut session, processing_deadline)
                .await?
            {
                return session.into_result(status);
            }
        }

        let releases = session
            .pending_lifecycle
            .iter()
            .filter(|change| matches!(change, RoleLifecycleChange::Release { .. }))
            .cloned()
            .collect::<Vec<_>>();
        for lifecycle in releases {
            if let RoleLifecycleChange::Release { logical_id, .. } = &lifecycle {
                session.state.roles.remove(logical_id);
                session.state.deleted_roles.remove(logical_id);
                session.state.pending_deletions.remove(logical_id);
                session.state.pending_creations.remove(logical_id);
                session.pending_lifecycle.retain(|change| change != &lifecycle);
                session.applied_lifecycle.push(lifecycle);
            }
        }

        session.into_result(RoleApplyStatus::Complete)
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

fn desired_role_attributes(definition: &DefinitionFile) -> Vec<(RoleLogicalId, RoleAttributes)> {
    definition
        .roles
        .iter()
        .map(|(logical_id, role_definition)| {
            (
                logical_id.clone(),
                compose_attributes(role_definition, &definition.settings_sets.role),
            )
        })
        .collect()
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
