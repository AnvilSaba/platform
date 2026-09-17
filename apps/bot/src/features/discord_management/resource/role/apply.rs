use std::time::{Duration, Instant};

use super::{AttributeChanges, Change, Plan, RolePlan, build_order_plan, build_plan, ordered_role_ids};
use crate::features::discord_management::apply::guild_lock::{GuildApplyLock, GuildApplyPermit};
use crate::features::discord_management::configuration::{
    DefinitionFile, PermissionVocabulary, PlanInput, StateFile, serialize_state,
};
use crate::features::discord_management::domain::ManagementError;
use crate::features::discord_management::ids::{GuildId, RoleId, RoleLogicalId};
use crate::features::discord_management::port::{
    RoleCatalog, RoleCreateOutcome, RoleDeleteOutcome, RoleLifecycleTarget, RolePositionUpdateOutcome,
    RolePositionUpdater, RoleSnapshot, RoleUpdate, RoleUpdateOutcome, RoleUpdater,
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
    CreationResponseUnknown,
    DeletionResponseUnknown,
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

struct ApplySession {
    _permit: GuildApplyPermit,
    state: StateFile,
    definition: DefinitionFile,
    catalog: RoleCatalog,
    applied: Plan,
    pending: Plan,
}

impl ApplySession {
    fn mark_applied(&mut self, logical_id: &RoleLogicalId) {
        let desired = self.pending.create_desired.get(logical_id).cloned();
        let change = self
            .pending
            .remove(logical_id)
            .expect("適用中の Role change は pending に存在します");
        self.applied.insert(logical_id.clone(), change);
        if let Some(desired) = desired {
            self.applied.create_desired.insert(logical_id.clone(), desired);
        }
    }

    fn into_result(self, status: RoleApplyStatus) -> Result<RoleApplyResult, ManagementError> {
        result(&self.state, status, self.applied, self.pending)
    }
}

enum ApplyPreparation {
    Finished(RoleApplyResult),
    Ready(Box<ApplySession>),
}

pub(crate) struct RoleApplyWorkflow<'a, S> {
    apply_lock: &'a GuildApplyLock,
    source: &'a S,
    vocabulary: &'a PermissionVocabulary,
}

impl<'a, S> RoleApplyWorkflow<'a, S> {
    pub(crate) fn new(apply_lock: &'a GuildApplyLock, source: &'a S, vocabulary: &'a PermissionVocabulary) -> Self {
        Self {
            apply_lock,
            source,
            vocabulary,
        }
    }
}

impl<S: RoleUpdater> RoleApplyWorkflow<'_, S> {
    pub(crate) async fn apply_role_updates(
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
        if session.pending.has_order() {
            return Err(ManagementError::InvalidState(
                "属性更新専用の apply_role_updates には Role の相対順序変更を含められません".to_owned(),
            ));
        }
        if session.pending.iter().any(|(_, change)| !change.is_update()) {
            return Err(ManagementError::InvalidState(
                "属性更新専用の apply_role_updates には Role の lifecycle 変更を含められません".to_owned(),
            ));
        }

        let updates = session
            .pending
            .iter()
            .filter_map(|(logical_id, change)| match change {
                Change::Update { discord_id, attributes } => {
                    Some((logical_id.clone(), *discord_id, attributes.clone()))
                }
                Change::Create | Change::Release { .. } | Change::Delete { .. } => None,
            })
            .collect::<Vec<_>>();
        for (logical_id, discord_id, attributes) in updates {
            if let Some(status) = apply_attribute_changes(
                self.source,
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

    async fn prepare_apply(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        processing_deadline: Instant,
    ) -> Result<ApplyPreparation, ManagementError> {
        let PlanInput { definition, state } = PlanInput::parse(definition_toml, state_json, guild_id, self.vocabulary)?;
        let Some(permit) = self.apply_lock.try_acquire(guild_id) else {
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
            _permit: permit,
            state,
            definition,
            catalog,
            applied,
            pending,
        })))
    }
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
    } else {
        return Ok(Some(RoleApplyStatus::ResponseUnknown));
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

    Ok(None)
}

async fn apply_order_changes<S: RolePositionUpdater>(
    source: &S,
    guild_id: &GuildId,
    session: &mut ApplySession,
    processing_deadline: Instant,
) -> Result<Option<RoleApplyStatus>, ManagementError> {
    let Some(planned_order) = build_order_plan(&session.definition, &session.state, &session.catalog)? else {
        session.applied.set_order(session.pending.take_order());
        return Ok(None);
    };
    if planned_order.updates.is_empty() {
        if planned_order.expected_order.is_empty() {
            // 作成前の managed Role を含む deferred order は、lifecycle が
            // state/catalog を確定するまで完了扱いにしません。
            session.pending.set_order(Some(planned_order));
            return Ok(Some(RoleApplyStatus::ReplanRequired));
        }
        session.applied.set_order(session.pending.take_order());
        return Ok(None);
    }
    if Instant::now() >= processing_deadline {
        return Ok(Some(RoleApplyStatus::DeadlineExceeded));
    }

    let mut position_error = None;
    let outcome = match tokio::time::timeout(
        processing_deadline.saturating_duration_since(Instant::now()),
        source.update_role_positions(guild_id, planned_order.updates.clone()),
    )
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            // Discord が一部だけ適用してからエラーを返す可能性があるため、
            // known error でも必ず最新 catalog を取得して pending order を
            // 再計画します。API エラー自体は status に保持します。
            position_error = Some(error.to_string());
            RolePositionUpdateOutcome::ResponseUnknown
        }
        Err(_) => RolePositionUpdateOutcome::ResponseUnknown,
    };

    let result_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
    session.catalog = match tokio::time::timeout(
        result_deadline.saturating_duration_since(Instant::now()),
        source.role_catalog(guild_id),
    )
    .await
    {
        Ok(Ok(catalog)) => catalog,
        Ok(Err(error)) => return Ok(Some(RoleApplyStatus::Failed(error.to_string()))),
        Err(_) => return Ok(Some(RoleApplyStatus::DeadlineExceeded)),
    };

    let actual_order = ordered_role_ids(&session.catalog, RoleId::new(session.state.guild_id.get()));
    if actual_order != planned_order.expected_order {
        session
            .pending
            .set_order(build_order_plan(&session.definition, &session.state, &session.catalog)?);
        return Ok(Some(
            position_error.map_or(RoleApplyStatus::ReplanRequired, RoleApplyStatus::Failed),
        ));
    }

    // 応答不明でも再取得した実順序が希望値なら、位置更新は確定成功とみなします。
    if let Some(error) = position_error {
        return Ok(Some(RoleApplyStatus::Failed(error)));
    }
    let _ = outcome;
    session.applied.set_order(session.pending.take_order());
    Ok(None)
}

impl<S: RoleLifecycleTarget> RoleApplyWorkflow<'_, S> {
    pub(crate) async fn apply_roles(
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

    pub(crate) async fn apply_roles_with_options(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &RolePlan,
        options: RoleApplyOptions,
        processing_deadline: Instant,
    ) -> Result<RoleApplyResult, ManagementError> {
        if !confirmed_plan.has_order() && confirmed_plan.iter().all(|(_, change)| change.is_update()) {
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
        if !options.allow_deletions && session.pending.contains_deletions() {
            return session.into_result(RoleApplyStatus::DeletionPermissionRequired);
        }

        let changes = session
            .pending
            .iter()
            .map(|(logical_id, change)| (logical_id.clone(), change.clone()))
            .collect::<Vec<_>>();
        for (logical_id, change) in changes {
            match change {
                Change::Create => {
                    if Instant::now() >= processing_deadline {
                        return session.into_result(RoleApplyStatus::DeadlineExceeded);
                    }
                    let create = match session.pending.create_desired.get(&logical_id).cloned().ok_or_else(|| {
                        ManagementError::InvalidState(format!(
                            "作成対象 Role {logical_id} の payload が plan にありません"
                        ))
                    }) {
                        Ok(create) => create,
                        Err(error) => return session.into_result(RoleApplyStatus::Failed(error.to_string())),
                    };
                    let outcome = match tokio::time::timeout(
                        processing_deadline.saturating_duration_since(Instant::now()),
                        self.source.create_role(&guild_id, create),
                    )
                    .await
                    {
                        Ok(Ok(outcome)) => outcome,
                        Ok(Err(error)) => return session.into_result(RoleApplyStatus::Failed(error.to_string())),
                        Err(_) => RoleCreateOutcome::ResponseUnknown,
                    };
                    let RoleCreateOutcome::Created(role_id) = outcome else {
                        return session.into_result(RoleApplyStatus::CreationResponseUnknown);
                    };
                    if session.state.roles.values().any(|existing_id| *existing_id == role_id) {
                        return session.into_result(RoleApplyStatus::Failed(format!(
                            "新しく作成した Role {role_id} は既存の対応と衝突しています"
                        )));
                    }
                    session.state.roles.insert(logical_id.clone(), role_id);
                    session.mark_applied(&logical_id);

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
                Change::Delete { discord_id } => {
                    if !options.allow_deletions {
                        return session.into_result(RoleApplyStatus::DeletionPermissionRequired);
                    }
                    if Instant::now() >= processing_deadline {
                        return session.into_result(RoleApplyStatus::DeadlineExceeded);
                    }
                    let exists = session.catalog.roles.iter().any(|role| role.id == discord_id);
                    if !exists {
                        session.state.roles.remove(&logical_id);
                        session.mark_applied(&logical_id);
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
                        return session.into_result(RoleApplyStatus::DeletionResponseUnknown);
                    }
                    session.state.roles.remove(&logical_id);
                    session.mark_applied(&logical_id);
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
                Change::Release { .. } => {
                    session.state.roles.remove(&logical_id);
                    session.mark_applied(&logical_id);
                }
                Change::Update { discord_id, attributes } => {
                    match apply_attribute_changes(
                        self.source,
                        &guild_id,
                        &logical_id,
                        discord_id,
                        &attributes,
                        &mut session,
                        processing_deadline,
                    )
                    .await
                    {
                        Ok(Some(status)) => return session.into_result(status),
                        Ok(None) => {}
                        Err(error) => {
                            return session.into_result(RoleApplyStatus::Failed(error.to_string()));
                        }
                    }
                }
            }
        }

        if session.pending.has_order() {
            if let Some(status) = apply_order_changes(self.source, &guild_id, &mut session, processing_deadline).await?
            {
                return session.into_result(status);
            }
        }

        session.into_result(RoleApplyStatus::Complete)
    }
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
