use std::time::{Duration, Instant};

use super::{
    AttributeChanges, Change, ChannelPlan, Plan, build_channel_plan_with_capabilities, compose_attributes,
    desired_channel_create_with_catalog,
};
use crate::features::discord_management::{
    apply::guild_lock::{GuildApplyLock, GuildApplyPermit},
    configuration::{ChannelKind, PermissionVocabulary, PlanInput, StateFile, serialize_state},
    domain::ManagementError,
    ids::{ChannelId, ChannelLogicalId, GuildId},
    port::{
        ChannelCatalog, ChannelCreateOutcome, ChannelDeleteOutcome, ChannelLifecycleTarget, ChannelSnapshot,
        ChannelSource, ChannelUpdate, ChannelUpdateOutcome, ChannelUpdateValue, ChannelUpdater,
    },
};

const RESULT_STATE_REFRESH_BUDGET: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ChannelApplyOptions {
    pub allow_deletions: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChannelApplyStatus {
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
pub(crate) struct ChannelApplyResult {
    pub status: ChannelApplyStatus,
    pub applied: ChannelPlan,
    pub pending: ChannelPlan,
    pub state_json: String,
}

struct ApplySession {
    _permit: GuildApplyPermit,
    state: StateFile,
    definition: crate::features::discord_management::configuration::DefinitionFile,
    catalog: ChannelCatalog,
    applied: Plan,
    pending: Plan,
}

impl ApplySession {
    fn mark_applied(&mut self, logical_id: &ChannelLogicalId) {
        let desired = self.pending.create_desired.get(logical_id).cloned();
        let change = self
            .pending
            .remove(logical_id)
            .expect("適用中の Channel change は pending に存在します");
        self.applied.insert(logical_id.clone(), change);
        if let Some(desired) = desired {
            self.applied.create_desired.insert(logical_id.clone(), desired);
        }
    }

    fn into_result(self, status: ChannelApplyStatus) -> Result<ChannelApplyResult, ManagementError> {
        Ok(ChannelApplyResult {
            status,
            applied: self.applied,
            pending: self.pending,
            state_json: serialize_state(&self.state)?,
        })
    }
}

enum ApplyPreparation {
    Finished(ChannelApplyResult),
    Ready(Box<ApplySession>),
}

pub(crate) struct ChannelApplyWorkflow<'a, S> {
    apply_lock: &'a GuildApplyLock,
    source: &'a S,
    vocabulary: &'a PermissionVocabulary,
}

impl<'a, S> ChannelApplyWorkflow<'a, S> {
    pub(crate) fn new(apply_lock: &'a GuildApplyLock, source: &'a S, vocabulary: &'a PermissionVocabulary) -> Self {
        Self {
            apply_lock,
            source,
            vocabulary,
        }
    }
}

impl<S: ChannelUpdater> ChannelApplyWorkflow<'_, S> {
    pub(crate) async fn apply_channel_updates(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &ChannelPlan,
        processing_deadline: Instant,
    ) -> Result<ChannelApplyResult, ManagementError> {
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
        if session.pending.iter().any(|(_, change)| !change.is_update()) {
            return Err(ManagementError::InvalidState(
                "属性更新専用の apply_channel_updates には Channel の lifecycle 変更を含められません".to_owned(),
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
        session.into_result(ChannelApplyStatus::Complete)
    }

    async fn prepare_apply(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &ChannelPlan,
        processing_deadline: Instant,
    ) -> Result<ApplyPreparation, ManagementError> {
        let PlanInput { definition, state } = PlanInput::parse(definition_toml, state_json, guild_id, self.vocabulary)?;
        let Some(permit) = self.apply_lock.try_acquire(guild_id) else {
            return Ok(ApplyPreparation::Finished(ChannelApplyResult {
                status: ChannelApplyStatus::GuildBusy,
                applied: Plan::default(),
                pending: confirmed_plan.clone(),
                state_json: serialize_state(&state)?,
            }));
        };
        if Instant::now() >= processing_deadline {
            return Ok(ApplyPreparation::Finished(ChannelApplyResult {
                status: ChannelApplyStatus::DeadlineExceeded,
                applied: Plan::default(),
                pending: confirmed_plan.clone(),
                state_json: serialize_state(&state)?,
            }));
        }
        let catalog = match tokio::time::timeout(
            processing_deadline.saturating_duration_since(Instant::now()),
            self.source.channel_catalog(&guild_id),
        )
        .await
        {
            Ok(Ok(catalog)) => catalog,
            Ok(Err(error)) => {
                return Ok(ApplyPreparation::Finished(ChannelApplyResult {
                    status: ChannelApplyStatus::Failed(error.to_string()),
                    applied: Plan::default(),
                    pending: confirmed_plan.clone(),
                    state_json: serialize_state(&state)?,
                }));
            }
            Err(_) => {
                return Ok(ApplyPreparation::Finished(ChannelApplyResult {
                    status: ChannelApplyStatus::DeadlineExceeded,
                    applied: Plan::default(),
                    pending: confirmed_plan.clone(),
                    state_json: serialize_state(&state)?,
                }));
            }
        };
        let can_manage_roles = match tokio::time::timeout(
            processing_deadline.saturating_duration_since(Instant::now()),
            self.source.can_manage_roles(&guild_id),
        )
        .await
        {
            Ok(Ok(can_manage_roles)) => can_manage_roles,
            Ok(Err(error)) => {
                return Ok(ApplyPreparation::Finished(ChannelApplyResult {
                    status: ChannelApplyStatus::Failed(error.to_string()),
                    applied: Plan::default(),
                    pending: confirmed_plan.clone(),
                    state_json: serialize_state(&state)?,
                }));
            }
            Err(_) => {
                return Ok(ApplyPreparation::Finished(ChannelApplyResult {
                    status: ChannelApplyStatus::DeadlineExceeded,
                    applied: Plan::default(),
                    pending: confirmed_plan.clone(),
                    state_json: serialize_state(&state)?,
                }));
            }
        };
        let current_plan = build_channel_plan_with_capabilities(&definition, &state, &catalog, can_manage_roles)?;
        if current_plan != *confirmed_plan {
            return Ok(ApplyPreparation::Finished(ChannelApplyResult {
                status: ChannelApplyStatus::ReplanRequired,
                applied: Plan::default(),
                pending: current_plan,
                state_json: serialize_state(&state)?,
            }));
        }
        Ok(ApplyPreparation::Ready(Box::new(ApplySession {
            _permit: permit,
            state,
            definition,
            catalog,
            applied: Plan::default(),
            pending: confirmed_plan.clone(),
        })))
    }
}

async fn apply_attribute_changes<S: ChannelUpdater>(
    source: &S,
    guild_id: &GuildId,
    logical_id: &ChannelLogicalId,
    channel_id: ChannelId,
    attributes: &AttributeChanges,
    session: &mut ApplySession,
    processing_deadline: Instant,
) -> Result<Option<ChannelApplyStatus>, ManagementError> {
    if session.pending.get(logical_id).is_none_or(|change| !change.is_update()) {
        return Ok(None);
    }
    if Instant::now() >= processing_deadline {
        return Ok(Some(ChannelApplyStatus::DeadlineExceeded));
    }
    let current = session
        .catalog
        .channels
        .iter()
        .find(|channel| channel.id == channel_id)
        .ok_or_else(|| {
            ManagementError::InvalidState(format!(
                "Channel {logical_id} の Snowflake {channel_id} が Guild に存在しません"
            ))
        })?;
    let update = attributes.to_update(current, &session.state)?;
    let outcome = match tokio::time::timeout(
        processing_deadline.saturating_duration_since(Instant::now()),
        source.update_channel(guild_id, &channel_id, update.clone()),
    )
    .await
    {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => return Ok(Some(ChannelApplyStatus::Failed(error.to_string()))),
        Err(_) => ChannelUpdateOutcome::ResponseUnknown,
    };
    if outcome == ChannelUpdateOutcome::Applied {
        session.mark_applied(logical_id);
    } else {
        return Ok(Some(ChannelApplyStatus::ResponseUnknown));
    }
    let result_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
    session.catalog = match tokio::time::timeout(
        result_deadline.saturating_duration_since(Instant::now()),
        source.channel_catalog(guild_id),
    )
    .await
    {
        Ok(Ok(catalog)) => catalog,
        Ok(Err(error)) => return Ok(Some(ChannelApplyStatus::Failed(error.to_string()))),
        Err(_) => {
            return Ok(Some(if outcome == ChannelUpdateOutcome::Applied {
                ChannelApplyStatus::DeadlineExceeded
            } else {
                ChannelApplyStatus::ResponseUnknown
            }));
        }
    };
    let matches = session
        .catalog
        .channels
        .iter()
        .find(|channel| channel.id == channel_id)
        .is_some_and(|channel| channel_matches_update(channel, &update));
    if !matches {
        return Ok(Some(if outcome == ChannelUpdateOutcome::ResponseUnknown {
            ChannelApplyStatus::ResponseUnknown
        } else {
            ChannelApplyStatus::Failed(format!("Channel {logical_id} の更新後の値が希望値と一致しません"))
        }));
    }
    Ok(None)
}

impl<S: ChannelLifecycleTarget> ChannelApplyWorkflow<'_, S> {
    #[cfg(test)]
    pub(crate) async fn apply_channels(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &ChannelPlan,
        processing_deadline: Instant,
    ) -> Result<ChannelApplyResult, ManagementError> {
        self.apply_channels_with_options(
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            ChannelApplyOptions::default(),
            processing_deadline,
        )
        .await
    }

    pub(crate) async fn apply_channels_with_options(
        &self,
        guild_id: GuildId,
        definition_toml: &str,
        state_json: &str,
        confirmed_plan: &ChannelPlan,
        options: ChannelApplyOptions,
        processing_deadline: Instant,
    ) -> Result<ChannelApplyResult, ManagementError> {
        if confirmed_plan.iter().all(|(_, change)| change.is_update()) {
            return self
                .apply_channel_updates(
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
            return session.into_result(ChannelApplyStatus::DeletionPermissionRequired);
        }

        let changes = ordered_changes(&session.pending, &session.definition);
        for (logical_id, change) in changes {
            match change {
                Change::Create => {
                    if Instant::now() >= processing_deadline {
                        return session.into_result(ChannelApplyStatus::DeadlineExceeded);
                    }
                    let desired = session
                        .definition
                        .channels
                        .get(&logical_id)
                        .expect("作成対象 Channel は definition に存在します");
                    let create = match desired_channel_create_with_catalog(
                        desired,
                        &logical_id,
                        &session.state,
                        &session.definition,
                        Some(&session.catalog),
                    ) {
                        Ok(create) => create,
                        Err(error) => return session.into_result(ChannelApplyStatus::Failed(error.to_string())),
                    };
                    let outcome = match tokio::time::timeout(
                        processing_deadline.saturating_duration_since(Instant::now()),
                        self.source.create_channel(&guild_id, create),
                    )
                    .await
                    {
                        Ok(Ok(outcome)) => outcome,
                        Ok(Err(error)) => return session.into_result(ChannelApplyStatus::Failed(error.to_string())),
                        Err(_) => ChannelCreateOutcome::ResponseUnknown,
                    };
                    let ChannelCreateOutcome::Created(channel_id) = outcome else {
                        return session.into_result(ChannelApplyStatus::CreationResponseUnknown);
                    };
                    if session
                        .state
                        .channels
                        .values()
                        .any(|existing_id| *existing_id == channel_id)
                    {
                        return session.into_result(ChannelApplyStatus::Failed(format!(
                            "新しく作成した Channel {channel_id} は既存の対応と衝突しています"
                        )));
                    }
                    session.state.channels.insert(logical_id.clone(), channel_id);
                    session.mark_applied(&logical_id);
                    match refresh_catalog(self.source, &guild_id, processing_deadline).await {
                        Ok(catalog) => session.catalog = catalog,
                        Err(status) => return session.into_result(status),
                    }
                }
                Change::Delete { discord_id } => {
                    if Instant::now() >= processing_deadline {
                        return session.into_result(ChannelApplyStatus::DeadlineExceeded);
                    }
                    let exists = session.catalog.channels.iter().any(|channel| channel.id == discord_id);
                    if !exists {
                        session.state.channels.remove(&logical_id);
                        session.mark_applied(&logical_id);
                        continue;
                    }
                    let outcome = match tokio::time::timeout(
                        processing_deadline.saturating_duration_since(Instant::now()),
                        self.source.delete_channel(&guild_id, &discord_id),
                    )
                    .await
                    {
                        Ok(Ok(outcome)) => outcome,
                        Ok(Err(error)) => {
                            let status = match error {
                                ManagementError::ChannelPermissionDenied(message) => {
                                    ChannelApplyStatus::DeletionPermissionDenied(message)
                                }
                                other => ChannelApplyStatus::Failed(other.to_string()),
                            };
                            return session.into_result(status);
                        }
                        Err(_) => ChannelDeleteOutcome::ResponseUnknown,
                    };
                    if outcome == ChannelDeleteOutcome::ResponseUnknown {
                        return session.into_result(ChannelApplyStatus::DeletionResponseUnknown);
                    }
                    // HTTP 204 は削除完了の確定応答なので、後続の再取得が失敗しても
                    // 対応表だけは成功結果として確定させます。
                    session.state.channels.remove(&logical_id);
                    session.mark_applied(&logical_id);
                    session.catalog = match refresh_catalog(self.source, &guild_id, processing_deadline).await {
                        Ok(catalog) => catalog,
                        Err(status) => return session.into_result(status),
                    };
                }
                Change::Release { .. } => {
                    session.state.channels.remove(&logical_id);
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
                            return session.into_result(ChannelApplyStatus::Failed(error.to_string()));
                        }
                    }
                }
            }
        }
        session.into_result(ChannelApplyStatus::Complete)
    }
}

async fn refresh_catalog<S: ChannelSource>(
    source: &S,
    guild_id: &GuildId,
    processing_deadline: Instant,
) -> Result<ChannelCatalog, ChannelApplyStatus> {
    let result_deadline = processing_deadline + RESULT_STATE_REFRESH_BUDGET;
    match tokio::time::timeout(
        result_deadline.saturating_duration_since(Instant::now()),
        source.channel_catalog(guild_id),
    )
    .await
    {
        Ok(Ok(catalog)) => Ok(catalog),
        Ok(Err(error)) => Err(ChannelApplyStatus::Failed(error.to_string())),
        Err(_) => Err(ChannelApplyStatus::DeadlineExceeded),
    }
}

fn ordered_changes(
    plan: &ChannelPlan,
    definition: &crate::features::discord_management::configuration::DefinitionFile,
) -> Vec<(ChannelLogicalId, Change)> {
    let mut changes = plan
        .iter()
        .map(|(logical_id, change)| (logical_id.clone(), change.clone()))
        .collect::<Vec<_>>();
    changes.sort_by_key(|(logical_id, change)| {
        let kind = definition
            .channels
            .get(logical_id)
            .and_then(|channel| compose_attributes(channel).kind);
        match change {
            Change::Create => match kind {
                Some(ChannelKind::Category) => 0,
                _ => 1,
            },
            Change::Delete { .. } => match kind {
                Some(ChannelKind::Category) => 3,
                _ => 2,
            },
            Change::Update { .. } => 1,
            Change::Release { .. } => 4,
        }
    });
    changes
}

fn channel_matches_update(channel: &ChannelSnapshot, update: &ChannelUpdate) -> bool {
    update.name.as_ref().is_none_or(|name| channel.name == *name)
        && optional_matches(channel.parent_id.as_ref(), &update.parent_id)
        && optional_matches(channel.topic.as_ref(), &update.topic)
        && update.nsfw.is_none_or(|nsfw| channel.nsfw == nsfw)
        && update
            .slowmode_seconds
            .is_none_or(|seconds| channel.slowmode_seconds == seconds)
        && optional_matches(
            channel.default_auto_archive_minutes.as_ref(),
            &update.default_auto_archive_minutes,
        )
        && optional_matches(
            channel.default_thread_slowmode_seconds.as_ref(),
            &update.default_thread_slowmode_seconds,
        )
        && update
            .overwrites
            .as_ref()
            .is_none_or(|overwrites| channel.overwrites == *overwrites)
}

fn optional_matches<T: PartialEq>(actual: Option<&T>, update: &ChannelUpdateValue<T>) -> bool {
    match update {
        ChannelUpdateValue::Keep => true,
        ChannelUpdateValue::Set(desired) => actual == Some(desired),
        ChannelUpdateValue::Clear => actual.is_none(),
    }
}
