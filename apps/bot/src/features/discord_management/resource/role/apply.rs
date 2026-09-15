use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use super::{AttributeChanges, Change, Plan, RolePlan, build_plan, compose_attributes};
use crate::features::discord_management::configuration::{
    Color, DefinitionFile, KnownPermission, PermissionVocabulary, PlanInput, RoleAttributes, StateFile, serialize_state,
};
use crate::features::discord_management::domain::ManagementError;
use crate::features::discord_management::ids::{GuildId, RoleId, RoleLogicalId};
use crate::features::discord_management::port::{
    RoleCatalog, RoleCreate, RoleCreateOutcome, RoleDeleteOutcome, RoleLifecycleTarget, RoleSnapshot, RoleUpdate,
    RoleUpdateOutcome, RoleUpdater,
};

const RESULT_STATE_REFRESH_BUDGET: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RoleApplyOptions {
    pub allow_deletions: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RoleApplyStatus {
    Complete,
    GuildBusy,
    ReplanRequired,
    DeadlineExceeded,
    DeletionPermissionRequired,
    DeletionPermissionDenied(String),
    DeletionVerificationPermissionDenied(String),
    CreationResponseUnknown,
    DeletionResponseUnknown,
    DeletionVerificationIndeterminate(String),
    Failed(String),
    ResponseUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoleApplyResult {
    pub status: RoleApplyStatus,
    pub applied: Plan,
    pub pending: Plan,
    pub state_json: String,
}

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
    applied: Plan,
    pending: Plan,
}

impl ApplySession {
    fn mark_applied(&mut self, logical_id: &RoleLogicalId) {
        let change = self
            .pending
            .remove(logical_id)
            .expect("適用中の Role change は pending に存在します");
        self.applied.insert(logical_id.clone(), change);
    }

    fn into_result(self, status: RoleApplyStatus) -> Result<RoleApplyResult, ManagementError> {
        result(&self.state, status, self.applied, self.pending)
    }
}

enum ApplyPreparation {
    Finished(RoleApplyResult),
    Ready(Box<ApplySession>),
}

pub(crate) async fn apply_role_updates<S: RoleUpdater>(
    source: &S,
    vocabulary: &PermissionVocabulary,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    let preparation = prepare_apply(
        source,
        vocabulary,
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
    if session.pending.iter().any(|(_, change)| !change.is_update()) {
        return Err(ManagementError::InvalidState(
            "属性更新専用の apply_role_updates には Role の lifecycle 変更を含められません".to_owned(),
        ));
    }

    let updates = session
        .pending
        .iter()
        .filter_map(|(logical_id, change)| match change {
            Change::Update { discord_id, attributes } => Some((logical_id.clone(), *discord_id, attributes.clone())),
            Change::Create { .. } | Change::Release { .. } | Change::Delete { .. } => None,
        })
        .collect::<Vec<_>>();
    for (logical_id, discord_id, attributes) in updates {
        if let Some(status) = apply_attribute_changes(
            source,
            &guild_id,
            &logical_id,
            discord_id,
            &attributes,
            &mut session,
            processing_deadline,
        )
        .await?
        {
            return session.into_result(status);
        }
    }

    session.into_result(RoleApplyStatus::Complete)
}

async fn prepare_apply<S: RoleUpdater>(
    source: &S,
    vocabulary: &PermissionVocabulary,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    processing_deadline: Instant,
) -> Result<ApplyPreparation, ManagementError> {
    let PlanInput { definition, state } = PlanInput::parse(definition_toml, state_json, guild_id, vocabulary)?;
    let Some(guard) = GuildApplyGuard::acquire(guild_id) else {
        return Ok(ApplyPreparation::Finished(result(
            &state,
            RoleApplyStatus::GuildBusy,
            Plan::default(),
            confirmed_plan.clone(),
        )?));
    };

    if Instant::now() >= processing_deadline {
        return Ok(ApplyPreparation::Finished(result(
            &state,
            RoleApplyStatus::DeadlineExceeded,
            Plan::default(),
            confirmed_plan.clone(),
        )?));
    }

    let applied = Plan::default();
    let pending = confirmed_plan.clone();
    let catalog = match tokio::time::timeout(
        processing_deadline.saturating_duration_since(Instant::now()),
        source.role_catalog(&guild_id),
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
            )?));
        }
        Err(_) => {
            return Ok(ApplyPreparation::Finished(result(
                &state,
                RoleApplyStatus::DeadlineExceeded,
                applied,
                pending,
            )?));
        }
    };
    let current_plan = build_plan(&definition, &state, &catalog)?;
    if current_plan != *confirmed_plan {
        return Ok(ApplyPreparation::Finished(result(
            &state,
            RoleApplyStatus::ReplanRequired,
            Plan::default(),
            current_plan,
        )?));
    }

    Ok(ApplyPreparation::Ready(Box::new(ApplySession {
        _guard: guard,
        state,
        definition,
        catalog,
        applied,
        pending,
    })))
}

async fn apply_attribute_changes<S: RoleUpdater>(
    source: &S,
    guild_id: &GuildId,
    logical_id: &RoleLogicalId,
    role_id: RoleId,
    attributes: &AttributeChanges,
    session: &mut ApplySession,
    processing_deadline: Instant,
) -> Result<Option<RoleApplyStatus>, ManagementError> {
    if session.pending.get(logical_id).is_none_or(|change| !change.is_update()) {
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
        let update = attributes.to_update(&actual.permissions);
        debug_assert!(
            !update.is_empty(),
            "空の Role 更新は AttributeChanges から生成されません"
        );
        update
    };

    let outcome = match tokio::time::timeout(
        processing_deadline.saturating_duration_since(Instant::now()),
        source.update_role(guild_id, &role_id, update.clone()),
    )
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => return Ok(Some(RoleApplyStatus::Failed(error.to_string()))),
        Err(_) => RoleUpdateOutcome::ResponseUnknown,
    };
    if outcome == RoleUpdateOutcome::Applied {
        session.mark_applied(logical_id);
    }

    let result_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
    session.catalog = match tokio::time::timeout(
        result_deadline.saturating_duration_since(Instant::now()),
        source.role_catalog(guild_id),
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
        session.mark_applied(logical_id);
    }
    Ok(None)
}

pub(crate) async fn apply_roles<S: RoleLifecycleTarget>(
    source: &S,
    vocabulary: &PermissionVocabulary,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    apply_roles_with_options(
        source,
        vocabulary,
        guild_id,
        definition_toml,
        state_json,
        confirmed_plan,
        RoleApplyOptions::default(),
        processing_deadline,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_roles_with_options<S: RoleLifecycleTarget>(
    source: &S,
    vocabulary: &PermissionVocabulary,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    options: RoleApplyOptions,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    if confirmed_plan.iter().all(|(_, change)| change.is_update()) {
        return apply_role_updates(
            source,
            vocabulary,
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            processing_deadline,
        )
        .await;
    }

    let preparation = prepare_apply(
        source,
        vocabulary,
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
    if !options.allow_deletions && session.pending.contains_deletions() {
        return session.into_result(RoleApplyStatus::DeletionPermissionRequired);
    }

    let desired_attributes = desired_role_attributes(&session.definition);
    let changes = session
        .pending
        .iter()
        .map(|(logical_id, change)| (logical_id.clone(), change.clone()))
        .collect::<Vec<_>>();
    for (logical_id, change) in changes {
        match change {
            Change::Create { .. } => {
                if Instant::now() >= processing_deadline {
                    return session.into_result(RoleApplyStatus::DeadlineExceeded);
                }
                let desired = desired_attributes
                    .get(&logical_id)
                    .expect("作成対象 Role は definition に存在します");
                let create = build_role_create(
                    desired,
                    &session.catalog.permission_names,
                    &session.catalog.default_permissions,
                    &logical_id,
                )?;
                session.state.pending_creations.insert(logical_id.clone());
                let outcome = match tokio::time::timeout(
                    processing_deadline.saturating_duration_since(Instant::now()),
                    source.create_role(&guild_id, create),
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
                session.mark_applied(&logical_id);

                let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                session.catalog = match tokio::time::timeout(
                    refresh_deadline.saturating_duration_since(Instant::now()),
                    source.role_catalog(&guild_id),
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
            Change::Delete { discord_id } => {
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
                    session.mark_applied(&logical_id);
                    continue;
                }
                let outcome = match tokio::time::timeout(
                    processing_deadline.saturating_duration_since(Instant::now()),
                    source.delete_role(&guild_id, &discord_id),
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
                        source.role_catalog(&guild_id),
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
                session.mark_applied(&logical_id);
                if outcome == RoleDeleteOutcome::Deleted {
                    let refresh_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
                    session.catalog = match tokio::time::timeout(
                        refresh_deadline.saturating_duration_since(Instant::now()),
                        source.role_catalog(&guild_id),
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
            Change::Release { .. } => {
                session.state.roles.remove(&logical_id);
                session.state.deleted_roles.remove(&logical_id);
                session.state.pending_deletions.remove(&logical_id);
                session.state.pending_creations.remove(&logical_id);
                session.mark_applied(&logical_id);
            }
            Change::Update { discord_id, attributes } => {
                if let Some(status) = apply_attribute_changes(
                    source,
                    &guild_id,
                    &logical_id,
                    discord_id,
                    &attributes,
                    &mut session,
                    processing_deadline,
                )
                .await?
                {
                    return session.into_result(status);
                }
            }
        }
    }

    session.into_result(RoleApplyStatus::Complete)
}

fn result(
    state: &StateFile,
    status: RoleApplyStatus,
    applied: Plan,
    pending: Plan,
) -> Result<RoleApplyResult, ManagementError> {
    Ok(RoleApplyResult {
        status,
        applied,
        pending,
        state_json: serialize_state(state)?,
    })
}

fn desired_role_attributes(definition: &DefinitionFile) -> BTreeMap<RoleLogicalId, RoleAttributes> {
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

fn build_role_create(
    desired: &RoleAttributes,
    permission_names: &BTreeSet<KnownPermission>,
    default_permissions: &BTreeMap<KnownPermission, bool>,
    logical_id: &RoleLogicalId,
) -> Result<RoleCreate, ManagementError> {
    let name = desired
        .name
        .as_ref()
        .ok_or_else(|| ManagementError::InvalidDefinition(format!("新しい Role {logical_id} には name が必要です")))?;
    let name = super::resolve(name, "new role".to_owned());
    let color = desired
        .color
        .as_ref()
        .map(|value| super::resolve(value, Color::default()))
        .unwrap_or_default();
    let hoist = desired
        .hoist
        .as_ref()
        .map(|value| super::resolve(value, false))
        .unwrap_or(false);
    let mentionable = desired
        .mentionable
        .as_ref()
        .map(|value| super::resolve(value, false))
        .unwrap_or(false);
    let mut permissions = permission_names
        .iter()
        .map(|permission| (permission.clone(), false))
        .collect::<BTreeMap<_, _>>();
    for (permission, value) in &desired.permissions {
        let default = *default_permissions
            .get(permission)
            .expect("RoleCatalog は既知の権限の Guild 既定値をすべて保持します");
        permissions.insert(permission.clone(), super::resolve(value, default));
    }
    Ok(RoleCreate {
        name,
        color,
        hoist,
        mentionable,
        permissions,
    })
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
