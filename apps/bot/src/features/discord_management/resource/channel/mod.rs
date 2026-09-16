use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    configuration::{
        ChannelAttributes, ChannelDefinition, ChannelKind, ChannelValue, DefinitionFile, KnownPermission,
        OverwriteValue, StateFile,
    },
    domain::ManagementError,
    ids::{ChannelId, ChannelLogicalId, RoleId},
    port::{
        ChannelCatalog, ChannelCreate, ChannelOverwritePermissions, ChannelOverwriteTarget, ChannelSnapshot,
        ChannelUpdate, ChannelUpdateValue,
    },
};

pub(crate) mod apply;

const DEFAULT_CHANNEL_NSFW: bool = false;
const DEFAULT_SLOWMODE_SECONDS: u16 = 0;
const DEFAULT_AUTO_ARCHIVE_MINUTES: Option<u16> = Some(1440);
const DEFAULT_THREAD_SLOWMODE_SECONDS: Option<u16> = Some(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValueChange<T> {
    current: T,
    desired: T,
}

impl<T> ValueChange<T> {
    fn between(current: T, desired: T) -> Option<Self>
    where
        T: PartialEq,
    {
        (current != desired).then_some(Self { current, desired })
    }

    pub(crate) fn current(&self) -> &T {
        &self.current
    }

    pub(crate) fn desired(&self) -> &T {
        &self.desired
    }
}

/// Optional な属性の差分を、値設定と解除の意図ごとに表します。
///
/// 変更がない場合は `AttributeChanges` 側の `Option` が `None` になり、
/// この型の値がある場合は必ず `Set` または `Clear` になります。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OptionalValueChange<T> {
    Set { current: Option<T>, desired: T },
    Clear { current: Option<T> },
}

impl<T> OptionalValueChange<T>
where
    T: PartialEq,
{
    fn between(current: Option<T>, desired: Option<T>) -> Option<Self> {
        match desired {
            Some(desired) if current.as_ref() != Some(&desired) => Some(Self::Set { current, desired }),
            None if current.is_some() => Some(Self::Clear { current }),
            _ => None,
        }
    }
}

impl<T> OptionalValueChange<T> {
    pub(crate) fn current(&self) -> &Option<T> {
        match self {
            Self::Set { current, .. } | Self::Clear { current } => current,
        }
    }

    pub(crate) fn desired(&self) -> Option<&T> {
        match self {
            Self::Set { desired, .. } => Some(desired),
            Self::Clear { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttributeChanges {
    intent_fingerprint: String,
    name: Option<ValueChange<String>>,
    parent: Option<OptionalValueChange<ChannelId>>,
    planned_parent: Option<PlannedParentChange>,
    topic: Option<OptionalValueChange<String>>,
    nsfw: Option<ValueChange<bool>>,
    slowmode_seconds: Option<ValueChange<u16>>,
    default_auto_archive_minutes: Option<OptionalValueChange<u16>>,
    default_thread_slowmode_seconds: Option<OptionalValueChange<u16>>,
    overwrites: BTreeMap<ChannelOverwriteTarget, BTreeMap<KnownPermission, ValueChange<OverwriteValue>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlannedParentChange {
    current: Option<ChannelId>,
    logical_id: ChannelLogicalId,
}

impl AttributeChanges {
    fn between(
        logical_id: &ChannelLogicalId,
        actual: &ChannelSnapshot,
        desired: &ChannelAttributes,
        state: &StateFile,
        planned_channel_creations: &BTreeSet<ChannelLogicalId>,
        can_manage_roles: bool,
    ) -> Result<Option<Self>, ManagementError> {
        let intent_fingerprint = fingerprint(&format!("{desired:?}"));
        let parent_target = desired
            .parent
            .as_ref()
            .map(|value| resolve_parent_target(value, logical_id, state, planned_channel_creations))
            .transpose()?;
        let name = desired
            .name
            .as_ref()
            .map(|value| resolve_name(value, logical_id))
            .transpose()?
            .and_then(|desired| ValueChange::between(actual.name.clone(), desired));
        let (parent, planned_parent) = match parent_target {
            Some(ParentTarget::Resolved(desired)) => (OptionalValueChange::between(actual.parent_id, desired), None),
            Some(ParentTarget::Planned(logical_id)) => (
                None,
                Some(PlannedParentChange {
                    current: actual.parent_id,
                    logical_id,
                }),
            ),
            None => (None, None),
        };
        let topic = desired
            .topic
            .as_ref()
            .map(resolve_topic)
            .transpose()?
            .and_then(|desired| OptionalValueChange::between(actual.topic.clone(), desired));
        let nsfw = desired
            .nsfw
            .as_ref()
            .map(|value| resolve_bool(value, DEFAULT_CHANNEL_NSFW, logical_id, "nsfw"))
            .transpose()?
            .and_then(|desired| ValueChange::between(actual.nsfw, desired));
        let slowmode_seconds = desired
            .slowmode_seconds
            .as_ref()
            .map(|value| resolve_u16(value, DEFAULT_SLOWMODE_SECONDS, logical_id, "slowmode_seconds"))
            .transpose()?
            .and_then(|desired| ValueChange::between(actual.slowmode_seconds, desired));
        let default_auto_archive_minutes = desired
            .default_auto_archive_minutes
            .as_ref()
            .map(|value| {
                resolve_optional_u16(
                    value,
                    DEFAULT_AUTO_ARCHIVE_MINUTES,
                    logical_id,
                    "default_auto_archive_minutes",
                )
            })
            .transpose()?
            .and_then(|desired| OptionalValueChange::between(actual.default_auto_archive_minutes, desired));
        let default_thread_slowmode_seconds = desired
            .default_thread_slowmode_seconds
            .as_ref()
            .map(|value| {
                resolve_optional_u16(
                    value,
                    DEFAULT_THREAD_SLOWMODE_SECONDS,
                    logical_id,
                    "default_thread_slowmode_seconds",
                )
            })
            .transpose()?
            .and_then(|desired| OptionalValueChange::between(actual.default_thread_slowmode_seconds, desired));
        let overwrites = build_overwrite_changes(logical_id, desired, actual, state)?;
        if !can_manage_roles && !overwrites.is_empty() {
            return Err(ManagementError::ChannelPermissionDenied(
                "Channel の permission overwrite 更新には MANAGE_ROLES 権限が必要です".to_owned(),
            ));
        }

        let changes = Self {
            intent_fingerprint,
            name,
            parent,
            planned_parent,
            topic,
            nsfw,
            slowmode_seconds,
            default_auto_archive_minutes,
            default_thread_slowmode_seconds,
            overwrites,
        };
        Ok((!changes.is_empty()).then_some(changes))
    }

    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.parent.is_none()
            && self.planned_parent.is_none()
            && self.topic.is_none()
            && self.nsfw.is_none()
            && self.slowmode_seconds.is_none()
            && self.default_auto_archive_minutes.is_none()
            && self.default_thread_slowmode_seconds.is_none()
            && self.overwrites.is_empty()
    }

    /// 更新要求のうち、現在値ではなく「何を実現したいか」だけを識別します。
    ///
    /// API 応答不明から再投入する際、対象の現在値が途中で変わっても同じ
    /// 意図として扱えるよう、`current` は fingerprint に含めません。
    pub(crate) fn intent_fingerprint(&self) -> String {
        self.intent_fingerprint.clone()
    }

    #[cfg(test)]
    pub(crate) fn name(&self) -> Option<&ValueChange<String>> {
        self.name.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn topic(&self) -> Option<&OptionalValueChange<String>> {
        self.topic.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn nsfw(&self) -> Option<&ValueChange<bool>> {
        self.nsfw.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn slowmode_seconds(&self) -> Option<&ValueChange<u16>> {
        self.slowmode_seconds.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn default_auto_archive_minutes(&self) -> Option<&OptionalValueChange<u16>> {
        self.default_auto_archive_minutes.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn default_thread_slowmode_seconds(&self) -> Option<&OptionalValueChange<u16>> {
        self.default_thread_slowmode_seconds.as_ref()
    }

    fn to_update(&self, actual: &ChannelSnapshot, state: &StateFile) -> Result<ChannelUpdate, ManagementError> {
        let mut overwrites = actual.overwrites.clone();
        for (target, permissions) in &self.overwrites {
            let target_permissions = overwrites.entry(target.clone()).or_default();
            for (permission, change) in permissions {
                match change.desired {
                    OverwriteValue::Clear => {
                        target_permissions.known.remove(permission);
                    }
                    value => {
                        target_permissions.known.insert(permission.clone(), value);
                    }
                }
            }
            if target_permissions.known.is_empty()
                && target_permissions.allow_unknown.is_empty()
                && target_permissions.deny_unknown.is_empty()
            {
                overwrites.remove(target);
            }
        }
        let parent_id = if let Some(change) = &self.planned_parent {
            let parent_id = state.channels.get(&change.logical_id).copied().ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Channel の親 {} の作成結果が state にありません",
                    change.logical_id
                ))
            })?;
            ChannelUpdateValue::Set(parent_id)
        } else {
            optional_update(self.parent.as_ref())
        };
        Ok(ChannelUpdate {
            name: self.name.as_ref().map(|change| change.desired.clone()),
            parent_id,
            topic: optional_update(self.topic.as_ref()),
            nsfw: self.nsfw.as_ref().map(|change| *change.desired()),
            slowmode_seconds: self.slowmode_seconds.as_ref().map(|change| *change.desired()),
            default_auto_archive_minutes: optional_update(self.default_auto_archive_minutes.as_ref()),
            default_thread_slowmode_seconds: optional_update(self.default_thread_slowmode_seconds.as_ref()),
            overwrites: (!self.overwrites.is_empty()).then_some(overwrites),
        })
    }

    fn render(&self, logical_id: &ChannelLogicalId, discord_id: &ChannelId, output: &mut String) {
        if let Some(change) = &self.name {
            render_value_change(
                output,
                logical_id,
                discord_id,
                "name",
                change.current(),
                change.desired(),
            );
        }
        if let Some(change) = &self.parent {
            render_optional_value_change(output, logical_id, discord_id, "parent", change);
        }
        if let Some(change) = &self.planned_parent {
            output.push_str(&format!(
                "- {} ({}) parent: {:?} -> planned:{}\n",
                logical_id, discord_id, change.current, change.logical_id
            ));
        }
        if let Some(change) = &self.topic {
            render_optional_value_change(output, logical_id, discord_id, "topic", change);
        }
        if let Some(change) = &self.nsfw {
            render_value_change(
                output,
                logical_id,
                discord_id,
                "nsfw",
                change.current(),
                change.desired(),
            );
        }
        if let Some(change) = &self.slowmode_seconds {
            render_value_change(
                output,
                logical_id,
                discord_id,
                "slowmode_seconds",
                change.current(),
                change.desired(),
            );
        }
        if let Some(change) = &self.default_auto_archive_minutes {
            render_optional_value_change(output, logical_id, discord_id, "default_auto_archive_minutes", change);
        }
        if let Some(change) = &self.default_thread_slowmode_seconds {
            render_optional_value_change(
                output,
                logical_id,
                discord_id,
                "default_thread_slowmode_seconds",
                change,
            );
        }
        for (target, permissions) in &self.overwrites {
            for (permission, change) in permissions {
                render_value_change(
                    output,
                    logical_id,
                    discord_id,
                    &format!("overwrites.{target:?}.{permission}"),
                    change.current(),
                    change.desired(),
                );
            }
        }
    }
}

fn optional_update<T: Clone>(change: Option<&OptionalValueChange<T>>) -> ChannelUpdateValue<T> {
    let Some(change) = change else {
        return ChannelUpdateValue::Keep;
    };
    match change {
        OptionalValueChange::Set { desired, .. } => ChannelUpdateValue::Set(desired.clone()),
        OptionalValueChange::Clear { .. } => ChannelUpdateValue::Clear,
    }
}

fn fingerprint(value: &str) -> String {
    // State の再投入間で安定する軽量な FNV-1a fingerprint です。機密性や
    // 改ざん検知は目的とせず、未完了の意図が同一かを識別するために使います。
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3_u64);
    }
    format!("{hash:016x}")
}

fn render_value_change<T: std::fmt::Debug>(
    output: &mut String,
    logical_id: &ChannelLogicalId,
    discord_id: &ChannelId,
    attribute: &str,
    current: &T,
    desired: &T,
) {
    output.push_str(&format!(
        "- {} ({}) {}: {:?} -> {:?}\n",
        logical_id, discord_id, attribute, current, desired
    ));
}

fn render_optional_value_change<T: std::fmt::Debug>(
    output: &mut String,
    logical_id: &ChannelLogicalId,
    discord_id: &ChannelId,
    attribute: &str,
    change: &OptionalValueChange<T>,
) {
    output.push_str(&format!(
        "- {} ({}) {}: {:?} -> {:?}\n",
        logical_id,
        discord_id,
        attribute,
        change.current(),
        change.desired()
    ));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Create {
        recreated: bool,
    },
    Update {
        discord_id: ChannelId,
        attributes: Box<AttributeChanges>,
    },
    Release {
        discord_id: ChannelId,
    },
    Delete {
        discord_id: ChannelId,
    },
}

#[cfg(test)]
pub(crate) type ChannelChange = Change;

impl Change {
    pub(crate) fn is_update(&self) -> bool {
        matches!(self, Self::Update { .. })
    }

    pub(crate) fn is_delete(&self) -> bool {
        matches!(self, Self::Delete { .. })
    }

    #[cfg(test)]
    pub(crate) fn recreated(&self) -> Option<bool> {
        match self {
            Self::Create { recreated } => Some(*recreated),
            Self::Update { .. } | Self::Release { .. } | Self::Delete { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn attributes(&self) -> Option<&AttributeChanges> {
        match self {
            Self::Update { attributes, .. } => Some(attributes),
            Self::Create { .. } | Self::Release { .. } | Self::Delete { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    changes: BTreeMap<ChannelLogicalId, Change>,
    create_desired: BTreeMap<ChannelLogicalId, ChannelAttributes>,
}

pub(crate) type ChannelPlan = Plan;

impl Plan {
    pub(crate) fn len(&self) -> usize {
        self.changes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&ChannelLogicalId, &Change)> {
        self.changes.iter()
    }

    pub(crate) fn get(&self, logical_id: &ChannelLogicalId) -> Option<&Change> {
        self.changes.get(logical_id)
    }

    pub(crate) fn contains_deletions(&self) -> bool {
        self.changes.values().any(Change::is_delete)
    }

    pub(super) fn insert(&mut self, logical_id: ChannelLogicalId, change: Change) {
        debug_assert!(self.changes.insert(logical_id, change).is_none());
    }

    fn insert_create(&mut self, logical_id: ChannelLogicalId, change: Change, desired: ChannelAttributes) {
        debug_assert!(matches!(change, Change::Create { .. }));
        debug_assert!(self.changes.insert(logical_id.clone(), change).is_none());
        self.create_desired.insert(logical_id, desired);
    }

    pub(super) fn remove(&mut self, logical_id: &ChannelLogicalId) -> Option<Change> {
        self.create_desired.remove(logical_id);
        self.changes.remove(logical_id)
    }

    pub(crate) fn render(&self) -> String {
        if self.is_empty() {
            return "変更はありません。\n".to_owned();
        }
        let mut output = String::from("Channel の変更計画\n\n");
        for (logical_id, change) in &self.changes {
            match change {
                Change::Create { recreated } => {
                    output.push_str(&format!(
                        "- {}: {logical_id}\n",
                        if *recreated { "再作成" } else { "新規作成" }
                    ));
                    if let Some(desired) = self.create_desired.get(logical_id) {
                        render_create_attributes(desired, &mut output);
                    }
                }
                Change::Update { discord_id, attributes } => {
                    attributes.render(logical_id, discord_id, &mut output)
                }
                Change::Release { discord_id } => {
                    output.push_str(&format!("- 管理解除: {logical_id} ({discord_id})\n"));
                }
                Change::Delete { discord_id } => output.push_str(&format!(
                    "- 削除: {logical_id} ({discord_id})\n  影響: Channel と配下の投稿・Thread が失われる可能性があります。\n"
                )),
            }
        }
        output
    }
}

fn render_create_attributes(attributes: &ChannelAttributes, output: &mut String) {
    output.push_str("  desired:\n");
    if let Some(kind) = attributes.kind {
        output.push_str(&format!("    type: {}\n", kind.as_str()));
    }
    if let Some(ChannelValue::Value(name)) = &attributes.name {
        output.push_str(&format!("    name: {name:?}\n"));
    }
    if let Some(parent) = &attributes.parent {
        match parent {
            ChannelValue::Value(parent) => output.push_str(&format!("    parent: {parent:?}\n")),
            ChannelValue::Clear => output.push_str("    parent: None\n"),
            ChannelValue::Default => {}
        }
    }
    if let Some(topic) = &attributes.topic {
        output.push_str(&format!("    topic: {}\n", render_topic_value(topic)));
    }
    if let Some(nsfw) = &attributes.nsfw {
        let nsfw = match nsfw {
            ChannelValue::Value(value) => *value,
            ChannelValue::Default | ChannelValue::Clear => DEFAULT_CHANNEL_NSFW,
        };
        output.push_str(&format!("    nsfw: {nsfw}\n"));
    }
    if let Some(slowmode) = &attributes.slowmode_seconds {
        output.push_str(&format!("    slowmode_seconds: {}\n", render_slowmode_value(slowmode)));
    }
    if let Some(auto_archive) = &attributes.default_auto_archive_minutes {
        output.push_str(&format!(
            "    default_auto_archive_minutes: {}\n",
            render_optional_u16_value(auto_archive, DEFAULT_AUTO_ARCHIVE_MINUTES)
        ));
    }
    if let Some(thread_slowmode) = &attributes.default_thread_slowmode_seconds {
        output.push_str(&format!(
            "    default_thread_slowmode_seconds: {}\n",
            render_optional_u16_value(thread_slowmode, DEFAULT_THREAD_SLOWMODE_SECONDS)
        ));
    }
    let overwrites = attributes
        .overwrites
        .iter()
        .filter_map(|(subject, permissions)| {
            let permissions = permissions
                .iter()
                .filter_map(|(permission, value)| {
                    let value = match value {
                        OverwriteValue::Allow => "allow",
                        OverwriteValue::Deny => "deny",
                        OverwriteValue::Clear => return None,
                    };
                    Some(format!("{permission}: {value}"))
                })
                .collect::<Vec<_>>();
            (!permissions.is_empty()).then_some(format!("{subject}: {{{}}}", permissions.join(", ")))
        })
        .collect::<Vec<_>>();
    if !overwrites.is_empty() {
        output.push_str(&format!("    overwrites: {{{}}}\n", overwrites.join(", ")));
    }
}

fn render_topic_value(value: &ChannelValue<String>) -> String {
    match value {
        ChannelValue::Value(value) if value.is_empty() => "None".to_owned(),
        ChannelValue::Value(value) => format!("Some({value:?})"),
        ChannelValue::Default | ChannelValue::Clear => "None".to_owned(),
    }
}

fn render_slowmode_value(value: &ChannelValue<u16>) -> u16 {
    match value {
        ChannelValue::Value(value) => *value,
        ChannelValue::Default | ChannelValue::Clear => DEFAULT_SLOWMODE_SECONDS,
    }
}

fn render_optional_u16_value(value: &ChannelValue<u16>, default: Option<u16>) -> String {
    match value {
        ChannelValue::Value(value) => format!("Some({value})"),
        ChannelValue::Default => format!("{default:?}"),
        ChannelValue::Clear => "None".to_owned(),
    }
}

pub(crate) fn compose_attributes(definition: &ChannelDefinition) -> ChannelAttributes {
    definition.attributes().clone()
}

/// 実環境の権限を明示して Channel の変更計画を組み立てます。
pub(crate) fn build_channel_plan_with_capabilities(
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &ChannelCatalog,
    can_manage_roles: bool,
) -> Result<ChannelPlan, ManagementError> {
    let actual = catalog
        .channels
        .iter()
        .map(|channel| (channel.id, channel))
        .collect::<BTreeMap<_, _>>();
    validate_pending_channel_state(definition, state)?;
    let planned_channel_creations = planned_channel_creations(definition, state);
    let mut plan = Plan::default();

    for (logical_id, desired) in &definition.channels {
        if desired.is_absent() {
            plan_absent_channel(logical_id, state, &actual, definition, &mut plan)?;
            continue;
        }
        if desired.is_reference() {
            let discord_id = state
                .channels
                .get(logical_id)
                .copied()
                .ok_or_else(|| ManagementError::InvalidState(format!("Channel {logical_id} の対応がありません")))?;
            ensure_actual_channel(logical_id, discord_id, &actual)?;
            continue;
        }

        let attributes = compose_attributes(desired);
        let kind = attributes.kind.ok_or_else(|| {
            ManagementError::InvalidDefinition(format!("管理対象 Channel {logical_id} には type が必要です"))
        })?;
        attributes.validate_for_kind(logical_id)?;
        validate_channel_parent(
            logical_id,
            kind,
            attributes.parent.as_ref(),
            definition,
            state,
            Some(&actual),
            &planned_channel_creations,
        )?;
        if state.deleted_channels.contains(logical_id) {
            validate_channel_creation(
                logical_id,
                &attributes,
                definition,
                state,
                Some(&actual),
                &planned_channel_creations,
            )?;
            plan.insert_create(
                logical_id.clone(),
                Change::Create { recreated: true },
                attributes.clone(),
            );
            continue;
        }
        let Some(discord_id) = state.channels.get(logical_id).copied() else {
            validate_channel_creation(
                logical_id,
                &attributes,
                definition,
                state,
                Some(&actual),
                &planned_channel_creations,
            )?;
            plan.insert_create(
                logical_id.clone(),
                Change::Create { recreated: false },
                attributes.clone(),
            );
            continue;
        };
        if state.pending_channel_deletions.contains(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の削除意図が未解決です"
            )));
        }
        let current = ensure_actual_channel(logical_id, discord_id, &actual)?;
        if current.kind != kind {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の種類変更はサポートしていません（{} -> {}）",
                current.kind.as_str(),
                kind.as_str()
            )));
        }
        if !current.manageable {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の Snowflake {discord_id} は Bot が管理できません"
            )));
        }
        if let Some(pending) = state.pending_channel_updates.get(logical_id)
            && (pending.discord_id != discord_id || pending.fingerprint != fingerprint(&format!("{attributes:?}")))
        {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の未完了更新 intent と今回の定義が一致しません"
            )));
        }
        if let Some(attributes) = AttributeChanges::between(
            logical_id,
            current,
            &attributes,
            state,
            &planned_channel_creations,
            can_manage_roles,
        )? {
            plan.insert(
                logical_id.clone(),
                Change::Update {
                    discord_id,
                    attributes: Box::new(attributes),
                },
            );
        }
    }

    for (logical_id, discord_id) in &state.channels {
        if definition.channels.contains_key(logical_id) {
            continue;
        }
        plan.insert(
            logical_id.clone(),
            Change::Release {
                discord_id: *discord_id,
            },
        );
    }

    Ok(plan)
}

fn planned_channel_creations(definition: &DefinitionFile, state: &StateFile) -> BTreeSet<ChannelLogicalId> {
    definition
        .channels
        .iter()
        .filter_map(|(logical_id, channel_definition)| {
            let is_managed_category = channel_definition.is_managed()
                && compose_attributes(channel_definition).kind == Some(ChannelKind::Category);
            if !is_managed_category {
                return None;
            }
            (state.deleted_channels.contains(logical_id) || !state.channels.contains_key(logical_id))
                .then_some(logical_id.clone())
        })
        .collect()
}

/// 応答不明の更新について、再取得した実構成が希望値へ到達していれば
/// 未完了 marker を解決します。
pub(crate) fn reconcile_pending_updates(
    definition: &DefinitionFile,
    state: &mut StateFile,
    catalog: &ChannelCatalog,
    can_manage_roles: bool,
) -> Result<(), ManagementError> {
    let actual = catalog
        .channels
        .iter()
        .map(|channel| (channel.id, channel))
        .collect::<BTreeMap<_, _>>();
    let planned_channel_creations = planned_channel_creations(definition, state);
    let pending_logical_ids = state.pending_channel_updates.keys().cloned().collect::<Vec<_>>();
    for logical_id in pending_logical_ids {
        let Some(channel_definition) = definition.channels.get(&logical_id) else {
            continue;
        };
        if !channel_definition.is_managed() {
            continue;
        }
        let Some(channel_id) = state.channels.get(&logical_id).copied() else {
            continue;
        };
        let Some(actual) = actual.get(&channel_id).copied() else {
            continue;
        };
        let desired = compose_attributes(channel_definition);
        let Some(kind) = desired.kind else {
            continue;
        };
        if actual.kind != kind || !actual.manageable {
            continue;
        }
        if AttributeChanges::between(
            &logical_id,
            actual,
            &desired,
            state,
            &planned_channel_creations,
            can_manage_roles,
        )?
        .is_none()
        {
            state.pending_channel_updates.remove(&logical_id);
        }
    }
    Ok(())
}

fn validate_pending_channel_state(definition: &DefinitionFile, state: &StateFile) -> Result<(), ManagementError> {
    if let Some(logical_id) = state.pending_channel_creations.iter().next() {
        return Err(ManagementError::InvalidState(format!(
            "Channel {logical_id} は作成結果不明のため、同じ定義で状態を確認する必要があります"
        )));
    }
    for logical_id in &state.pending_channel_deletions {
        let Some(channel) = definition.channels.get(logical_id) else {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の削除意図が未解決のため、定義を変更できません"
            )));
        };
        if !channel.is_absent() {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の削除意図が未解決です"
            )));
        }
    }
    for logical_id in state.pending_channel_updates.keys() {
        let Some(channel) = definition.channels.get(logical_id) else {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の更新意図が未解決のため、定義を変更できません"
            )));
        };
        if !channel.is_managed() {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の更新意図が未解決です"
            )));
        }
    }
    Ok(())
}

fn plan_absent_channel(
    logical_id: &ChannelLogicalId,
    state: &StateFile,
    actual: &BTreeMap<ChannelId, &ChannelSnapshot>,
    definition: &DefinitionFile,
    plan: &mut Plan,
) -> Result<(), ManagementError> {
    let Some(discord_id) = state.channels.get(logical_id).copied() else {
        return Ok(());
    };
    if state.deleted_channels.contains(logical_id) {
        return Ok(());
    }
    let current = if state.pending_channel_deletions.contains(logical_id) {
        actual.get(&discord_id).copied()
    } else {
        Some(ensure_actual_channel(logical_id, discord_id, actual)?)
    };
    if let Some(current) = current {
        if !current.manageable {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の Snowflake {discord_id} は Bot が管理できないため削除できません"
            )));
        }
        if current.kind == ChannelKind::Category {
            validate_category_children(logical_id, discord_id, definition, state, actual)?;
        }
    }
    plan.insert(logical_id.clone(), Change::Delete { discord_id });
    Ok(())
}

fn validate_category_children(
    category_logical_id: &ChannelLogicalId,
    category_id: ChannelId,
    definition: &DefinitionFile,
    state: &StateFile,
    actual: &BTreeMap<ChannelId, &ChannelSnapshot>,
) -> Result<(), ManagementError> {
    for child in actual.values().filter(|channel| channel.parent_id == Some(category_id)) {
        let child_logical_id = state
            .channels
            .iter()
            .find_map(|(logical_id, discord_id)| (*discord_id == child.id).then_some(logical_id));
        let Some(child_logical_id) = child_logical_id else {
            return Err(ManagementError::InvalidState(format!(
                "Category {category_logical_id} の削除前に、管理外の子 Channel {} の移動または親解除を明示してください",
                child.id
            )));
        };
        let Some(child_definition) = definition.channels.get(child_logical_id) else {
            return Err(ManagementError::InvalidDefinition(format!(
                "Category {category_logical_id} の削除前に子 Channel {child_logical_id} の移動または削除を明示してください"
            )));
        };
        if child_definition.is_absent() {
            continue;
        }
        if child_definition.is_reference() {
            return Err(ManagementError::InvalidDefinition(format!(
                "Category {category_logical_id} の削除時、参照専用の子 Channel {child_logical_id} の移動または親解除が必要です"
            )));
        }
        let attributes = compose_attributes(child_definition);
        if matches!(attributes.parent, None | Some(ChannelValue::Default))
            || matches!(attributes.parent, Some(ChannelValue::Value(ref parent)) if parent == category_logical_id)
        {
            return Err(ManagementError::InvalidDefinition(format!(
                "Category {category_logical_id} の削除時、子 Channel {child_logical_id} の parent を変更または clear してください"
            )));
        }
    }
    Ok(())
}

fn ensure_actual_channel<'a>(
    logical_id: &ChannelLogicalId,
    discord_id: ChannelId,
    actual: &'a BTreeMap<ChannelId, &'a ChannelSnapshot>,
) -> Result<&'a ChannelSnapshot, ManagementError> {
    actual.get(&discord_id).copied().ok_or_else(|| {
        ManagementError::InvalidState(format!(
            "Channel {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
        ))
    })
}

fn validate_channel_parent(
    logical_id: &ChannelLogicalId,
    kind: ChannelKind,
    parent: Option<&ChannelValue<ChannelLogicalId>>,
    definition: &DefinitionFile,
    state: &StateFile,
    actual: Option<&BTreeMap<ChannelId, &ChannelSnapshot>>,
    planned_channel_creations: &BTreeSet<ChannelLogicalId>,
) -> Result<(), ManagementError> {
    let Some(parent) = parent else {
        return Ok(());
    };
    if kind == ChannelKind::Category {
        return Err(ManagementError::InvalidDefinition(format!(
            "Category {logical_id} には parent を指定できません"
        )));
    }
    let ChannelValue::Value(parent_logical_id) = parent else {
        return Ok(());
    };
    let Some(parent_definition) = definition.channels.get(parent_logical_id) else {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の親 {parent_logical_id} の宣言がありません"
        )));
    };
    if parent_definition.is_absent() {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の親 {parent_logical_id} は削除宣言です"
        )));
    }
    let declared_kind = compose_attributes(parent_definition).kind;
    if declared_kind == Some(ChannelKind::Text) {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の親 {parent_logical_id} は Category である必要があります"
        )));
    }
    let parent_discord_id = state.channels.get(parent_logical_id).copied();
    if declared_kind.is_none() && !parent_definition.is_reference() {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の親 {parent_logical_id} の type がありません"
        )));
    }
    if planned_channel_creations.contains(parent_logical_id) {
        return Ok(());
    }
    if parent_definition.is_reference() && parent_discord_id.is_none() {
        return Err(ManagementError::InvalidState(format!(
            "Channel {logical_id} の親 {parent_logical_id} の対応がありません"
        )));
    }
    if let Some(parent_discord_id) = parent_discord_id
        && let Some(actual) = actual
    {
        let parent_actual = actual.get(&parent_discord_id).ok_or_else(|| {
            ManagementError::InvalidState(format!(
                "Channel {logical_id} の親 {parent_logical_id} の Snowflake {parent_discord_id} が Guild から予期せず消失しています"
            ))
        })?;
        if parent_actual.kind != ChannelKind::Category {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の親 {parent_logical_id} は Category である必要があります"
            )));
        }
    }
    Ok(())
}

fn validate_channel_creation(
    logical_id: &ChannelLogicalId,
    attributes: &ChannelAttributes,
    definition: &DefinitionFile,
    state: &StateFile,
    actual: Option<&BTreeMap<ChannelId, &ChannelSnapshot>>,
    planned_channel_creations: &BTreeSet<ChannelLogicalId>,
) -> Result<(), ManagementError> {
    let kind = attributes.kind.ok_or_else(|| {
        ManagementError::InvalidDefinition(format!("新しい Channel {logical_id} には type が必要です"))
    })?;
    let Some(ChannelValue::Value(name)) = attributes.name.as_ref() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "新しい Channel {logical_id} には name の具体値が必要です"
        )));
    };
    if name.is_empty() {
        return Err(ManagementError::InvalidDefinition(format!(
            "新しい Channel {logical_id} の name は空にできません"
        )));
    }
    validate_channel_parent(
        logical_id,
        kind,
        attributes.parent.as_ref(),
        definition,
        state,
        actual,
        planned_channel_creations,
    )?;
    if let Some(ChannelValue::Value(parent)) = &attributes.parent
        && definition
            .channels
            .get(parent)
            .is_some_and(ChannelDefinition::is_reference)
        && !state.channels.contains_key(parent)
    {
        return Err(ManagementError::InvalidState(format!(
            "Channel {logical_id} の親 {parent} を作成または bind できません"
        )));
    }
    Ok(())
}

fn resolve_name(value: &ChannelValue<String>, logical_id: &ChannelLogicalId) -> Result<String, ManagementError> {
    match value {
        ChannelValue::Value(value) if !value.is_empty() => Ok(value.clone()),
        ChannelValue::Value(_) => Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の name は空にできません"
        ))),
        ChannelValue::Default => Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の name に default は指定できません"
        ))),
        ChannelValue::Clear => Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の name は解除できません"
        ))),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ParentTarget {
    Resolved(Option<ChannelId>),
    Planned(ChannelLogicalId),
}

fn resolve_parent_target(
    value: &ChannelValue<ChannelLogicalId>,
    logical_id: &ChannelLogicalId,
    state: &StateFile,
    planned_channel_creations: &BTreeSet<ChannelLogicalId>,
) -> Result<ParentTarget, ManagementError> {
    match value {
        ChannelValue::Value(parent) => {
            if planned_channel_creations.contains(parent) {
                Ok(ParentTarget::Planned(parent.clone()))
            } else if let Some(parent_id) = state.channels.get(parent).copied() {
                Ok(ParentTarget::Resolved(Some(parent_id)))
            } else {
                Err(ManagementError::InvalidState(format!(
                    "Channel {logical_id} の親 {parent} の対応がありません"
                )))
            }
        }
        ChannelValue::Clear => Ok(ParentTarget::Resolved(None)),
        ChannelValue::Default => Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の parent に default は指定できません"
        ))),
    }
}

fn resolve_parent(
    value: &ChannelValue<ChannelLogicalId>,
    logical_id: &ChannelLogicalId,
    state: &StateFile,
) -> Result<Option<ChannelId>, ManagementError> {
    match resolve_parent_target(value, logical_id, state, &BTreeSet::new())? {
        ParentTarget::Resolved(parent_id) => Ok(parent_id),
        ParentTarget::Planned(parent) => Err(ManagementError::InvalidState(format!(
            "Channel {logical_id} の親 {parent} の作成結果が state にありません"
        ))),
    }
}

fn resolve_topic(value: &ChannelValue<String>) -> Result<Option<String>, ManagementError> {
    Ok(match value {
        ChannelValue::Value(value) if value.is_empty() => None,
        ChannelValue::Value(value) => Some(value.clone()),
        ChannelValue::Default | ChannelValue::Clear => None,
    })
}

fn resolve_bool(
    value: &ChannelValue<bool>,
    default: bool,
    _logical_id: &ChannelLogicalId,
    _attribute: &str,
) -> Result<bool, ManagementError> {
    Ok(match value {
        ChannelValue::Value(value) => *value,
        ChannelValue::Default => default,
        ChannelValue::Clear => {
            return Err(ManagementError::InvalidDefinition(
                "この属性は解除できません".to_owned(),
            ));
        }
    })
}

fn resolve_u16(
    value: &ChannelValue<u16>,
    default: u16,
    _logical_id: &ChannelLogicalId,
    _attribute: &str,
) -> Result<u16, ManagementError> {
    Ok(match value {
        ChannelValue::Value(value) => *value,
        ChannelValue::Default => default,
        ChannelValue::Clear => 0,
    })
}

fn resolve_optional_u16(
    value: &ChannelValue<u16>,
    default: Option<u16>,
    _logical_id: &ChannelLogicalId,
    _attribute: &str,
) -> Result<Option<u16>, ManagementError> {
    Ok(match value {
        ChannelValue::Value(value) => Some(*value),
        ChannelValue::Default => default,
        ChannelValue::Clear => None,
    })
}

fn build_overwrite_changes(
    logical_id: &ChannelLogicalId,
    desired: &ChannelAttributes,
    actual: &ChannelSnapshot,
    state: &StateFile,
) -> Result<BTreeMap<ChannelOverwriteTarget, BTreeMap<KnownPermission, ValueChange<OverwriteValue>>>, ManagementError> {
    let mut changes = BTreeMap::new();
    for (subject, permissions) in &desired.overwrites {
        let target = resolve_overwrite_target(subject, logical_id, state)?;
        let current_permissions = actual.overwrites.get(&target);
        let mut target_changes = BTreeMap::new();
        for (permission, desired_value) in permissions {
            let current = current_permissions
                .and_then(|permissions| permissions.known.get(permission).copied())
                .unwrap_or(OverwriteValue::Clear);
            if let Some(change) = ValueChange::between(current, *desired_value) {
                target_changes.insert(permission.clone(), change);
            }
        }
        if !target_changes.is_empty() {
            changes.insert(target, target_changes);
        }
    }
    Ok(changes)
}

fn resolve_overwrite_target(
    subject: &str,
    logical_id: &ChannelLogicalId,
    state: &StateFile,
) -> Result<ChannelOverwriteTarget, ManagementError> {
    if subject == "everyone" {
        return Ok(ChannelOverwriteTarget::Everyone);
    }
    if let Some(role) = subject.strip_prefix("role:") {
        let role = super::super::ids::RoleLogicalId::parse(role).map_err(|error| {
            ManagementError::InvalidDefinition(format!("Channel {logical_id} の {subject} が不正です: {error}"))
        })?;
        let discord_id = if role == super::super::configuration::everyone_logical_id() {
            RoleId::new(state.guild_id.get())
        } else {
            state.roles.get(&role).copied().ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Channel {logical_id} の権限対象 Role {role} の対応がありません"
                ))
            })?
        };
        return Ok(ChannelOverwriteTarget::Role(discord_id));
    }
    if let Some(member) = subject.strip_prefix("member:") {
        let member = super::super::ids::MemberLogicalId::parse(member).map_err(|error| {
            ManagementError::InvalidDefinition(format!("Channel {logical_id} の {subject} が不正です: {error}"))
        })?;
        let discord_id = state.members.get(&member).copied().ok_or_else(|| {
            ManagementError::InvalidState(format!(
                "Channel {logical_id} の権限対象 Member {member} の対応がありません"
            ))
        })?;
        return Ok(ChannelOverwriteTarget::Member(discord_id));
    }
    Err(ManagementError::InvalidDefinition(format!(
        "Channel {logical_id} の権限対象 {subject} が不正です"
    )))
}

pub(crate) fn desired_channel_create_with_catalog(
    definition: &ChannelDefinition,
    logical_id: &ChannelLogicalId,
    state: &StateFile,
    definition_file: &DefinitionFile,
    catalog: Option<&ChannelCatalog>,
) -> Result<ChannelCreate, ManagementError> {
    let attributes = compose_attributes(definition);
    let actual = catalog.map(|catalog| {
        catalog
            .channels
            .iter()
            .map(|channel| (channel.id, channel))
            .collect::<BTreeMap<_, _>>()
    });
    let planned_channel_creations = planned_channel_creations(definition_file, state);
    validate_channel_creation(
        logical_id,
        &attributes,
        definition_file,
        state,
        actual.as_ref(),
        &planned_channel_creations,
    )?;
    let kind = attributes.kind.expect("作成前に Channel kind を検証します");
    let name = resolve_name(
        attributes.name.as_ref().expect("作成前に Channel name を検証します"),
        logical_id,
    )?;
    let parent_id = match attributes.parent.as_ref() {
        Some(value) => resolve_parent(value, logical_id, state)?,
        None => None,
    };
    let topic = match attributes.topic.as_ref() {
        Some(value) => resolve_topic(value)?,
        None => None,
    };
    let nsfw = attributes
        .nsfw
        .as_ref()
        .map(|value| resolve_bool(value, DEFAULT_CHANNEL_NSFW, logical_id, "nsfw"))
        .transpose()?
        .unwrap_or(DEFAULT_CHANNEL_NSFW);
    let slowmode_seconds = attributes
        .slowmode_seconds
        .as_ref()
        .map(|value| resolve_u16(value, DEFAULT_SLOWMODE_SECONDS, logical_id, "slowmode_seconds"))
        .transpose()?
        .unwrap_or(DEFAULT_SLOWMODE_SECONDS);
    let default_auto_archive_minutes = match attributes.default_auto_archive_minutes.as_ref() {
        Some(value) => resolve_optional_u16(
            value,
            DEFAULT_AUTO_ARCHIVE_MINUTES,
            logical_id,
            "default_auto_archive_minutes",
        )?,
        None => DEFAULT_AUTO_ARCHIVE_MINUTES,
    };
    let default_thread_slowmode_seconds = match attributes.default_thread_slowmode_seconds.as_ref() {
        Some(value) => resolve_optional_u16(
            value,
            DEFAULT_THREAD_SLOWMODE_SECONDS,
            logical_id,
            "default_thread_slowmode_seconds",
        )?,
        None => DEFAULT_THREAD_SLOWMODE_SECONDS,
    };
    let mut overwrites = BTreeMap::new();
    for (subject, permissions) in &attributes.overwrites {
        let target = resolve_overwrite_target(subject, logical_id, state)?;
        let permissions = permissions
            .iter()
            .filter_map(|(permission, value)| {
                (!matches!(value, OverwriteValue::Clear)).then_some((permission.clone(), *value))
            })
            .collect::<BTreeMap<_, _>>();
        if !permissions.is_empty() {
            overwrites.insert(
                target,
                ChannelOverwritePermissions {
                    known: permissions,
                    ..ChannelOverwritePermissions::default()
                },
            );
        }
    }
    Ok(ChannelCreate {
        kind,
        name,
        parent_id,
        topic,
        nsfw,
        slowmode_seconds,
        default_auto_archive_minutes,
        default_thread_slowmode_seconds,
        overwrites,
    })
}
