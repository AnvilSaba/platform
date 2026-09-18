use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    configuration::{
        ChannelAttributes, ChannelDefinition, ChannelKind, ChannelValue, DefinitionFile, KnownPermission,
        OverwriteTarget, OverwriteValue, StateFile,
    },
    domain::ManagementError,
    ids::{ChannelId, ChannelLogicalId, RoleId},
    port::{
        ChannelCatalog, ChannelCreate, ChannelOverwritePermissions, ChannelOverwriteTarget, ChannelPositionUpdate,
        ChannelSnapshot, ChannelUpdate, ChannelUpdateValue,
    },
};

pub(crate) mod apply;

use super::{compare_position_then_id, display_quoted_string, render_change_line, stable_relative_order};

const DEFAULT_CHANNEL_NSFW: bool = false;
const DEFAULT_SLOWMODE_SECONDS: u16 = 0;
const DEFAULT_AUTO_ARCHIVE_MINUTES: Option<u16> = Some(1440);
const DEFAULT_THREAD_SLOWMODE_SECONDS: u16 = 0;

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

/// Nullable な属性の差分を、値設定と解除の意図ごとに表します。
///
/// 変更がない場合は `AttributeChanges` 側の外側の `Option` が `None` になり、
/// この型の値がある場合は必ず `Set` または `Clear` になります。内側の
/// `Option` は、属性の現在値が未設定であることを表します。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NullableValueChange<T> {
    Set { current: Option<T>, desired: T },
    Clear { current: Option<T> },
}

impl<T> NullableValueChange<T>
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

impl<T> NullableValueChange<T> {
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
    name: Option<ValueChange<String>>,
    parent: Option<NullableValueChange<ChannelId>>,
    planned_parent: Option<PlannedParentChange>,
    topic: Option<NullableValueChange<String>>,
    nsfw: Option<ValueChange<bool>>,
    slowmode_seconds: Option<ValueChange<u16>>,
    default_auto_archive_minutes: Option<NullableValueChange<u16>>,
    default_thread_slowmode_seconds: Option<ValueChange<u16>>,
    overwrites: BTreeMap<ChannelOverwriteTarget, BTreeMap<KnownPermission, ValueChange<OverwriteValue>>>,
    /// 同期時に子へ送る Category の完成形です。空の map も有効な更新値です。
    synced_overwrites: Option<BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>>,
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
        synced_overwrites: Option<BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>>,
    ) -> Result<Option<Self>, ManagementError> {
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
            Some(ParentTarget::Resolved(desired)) => (NullableValueChange::between(actual.parent_id, desired), None),
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
            .and_then(|desired| NullableValueChange::between(actual.topic.clone(), desired));
        let nsfw = desired
            .nsfw
            .as_ref()
            .map(|value| resolve_bool(value, DEFAULT_CHANNEL_NSFW))
            .transpose()?
            .and_then(|desired| ValueChange::between(actual.nsfw, desired));
        let slowmode_seconds = desired
            .slowmode_seconds
            .as_ref()
            .map(|value| resolve_u16(value, DEFAULT_SLOWMODE_SECONDS))
            .transpose()?
            .and_then(|desired| ValueChange::between(actual.slowmode_seconds, desired));
        let default_auto_archive_minutes = desired
            .default_auto_archive_minutes
            .as_ref()
            .map(|value| resolve_nullable_u16(value, DEFAULT_AUTO_ARCHIVE_MINUTES))
            .transpose()?
            .and_then(|desired| NullableValueChange::between(actual.default_auto_archive_minutes, desired));
        let default_thread_slowmode_seconds = desired
            .default_thread_slowmode_seconds
            .as_ref()
            .map(|value| resolve_u16(value, DEFAULT_THREAD_SLOWMODE_SECONDS))
            .transpose()?
            .and_then(|desired| ValueChange::between(actual.default_thread_slowmode_seconds, desired));
        let overwrites = if synced_overwrites.is_some() {
            BTreeMap::new()
        } else {
            build_overwrite_changes(logical_id, desired, actual, state)?
        };
        let synced_overwrites = synced_overwrites.filter(|overwrites| actual.overwrites != *overwrites);
        if !can_manage_roles && (!overwrites.is_empty() || synced_overwrites.is_some()) {
            return Err(ManagementError::ChannelPermissionDenied(
                "Channel の permission overwrite 更新には MANAGE_ROLES 権限が必要です".to_owned(),
            ));
        }

        let changes = Self {
            name,
            parent,
            planned_parent,
            topic,
            nsfw,
            slowmode_seconds,
            default_auto_archive_minutes,
            default_thread_slowmode_seconds,
            overwrites,
            synced_overwrites,
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
            && self.synced_overwrites.is_none()
    }

    #[cfg(test)]
    pub(crate) fn name(&self) -> Option<&ValueChange<String>> {
        self.name.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn topic(&self) -> Option<&NullableValueChange<String>> {
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
    pub(crate) fn default_auto_archive_minutes(&self) -> Option<&NullableValueChange<u16>> {
        self.default_auto_archive_minutes.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn default_thread_slowmode_seconds(&self) -> Option<&ValueChange<u16>> {
        self.default_thread_slowmode_seconds.as_ref()
    }

    fn to_update(&self, actual: &ChannelSnapshot, state: &StateFile) -> Result<ChannelUpdate, ManagementError> {
        let overwrites = if let Some(synced_overwrites) = &self.synced_overwrites {
            synced_overwrites.clone()
        } else {
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
            overwrites
        };
        let parent_id = if let Some(change) = &self.planned_parent {
            let parent_id = state.channels.get(&change.logical_id).copied().ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Channel の親 {} の作成結果が state にありません",
                    change.logical_id
                ))
            })?;
            ChannelUpdateValue::Set(parent_id)
        } else {
            nullable_update(self.parent.as_ref())
        };
        Ok(ChannelUpdate {
            name: self.name.as_ref().map(|change| change.desired.clone()),
            parent_id,
            topic: nullable_update(self.topic.as_ref()),
            nsfw: self.nsfw.as_ref().map(|change| *change.desired()),
            slowmode_seconds: self.slowmode_seconds.as_ref().map(|change| *change.desired()),
            default_auto_archive_minutes: nullable_update(self.default_auto_archive_minutes.as_ref()),
            default_thread_slowmode_seconds: self
                .default_thread_slowmode_seconds
                .as_ref()
                .map(|change| *change.desired()),
            overwrites: (self.synced_overwrites.is_some() || !self.overwrites.is_empty()).then_some(overwrites),
        })
    }

    fn render(&self, logical_id: &ChannelLogicalId, discord_id: &ChannelId, output: &mut String) {
        if let Some(change) = &self.name {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "name",
                display_quoted_string(change.current()),
                display_quoted_string(change.desired()),
            );
        }
        if let Some(change) = &self.parent {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "parent",
                display_nullable(change.current().as_ref()),
                display_nullable(change.desired()),
            );
        }
        if let Some(change) = &self.planned_parent {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "parent",
                display_nullable(change.current.as_ref()),
                format!("planned:{}", change.logical_id),
            );
        }
        if let Some(change) = &self.topic {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "topic",
                display_nullable_string(change.current().as_ref()),
                display_nullable_string(change.desired()),
            );
        }
        if let Some(change) = &self.nsfw {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "nsfw",
                change.current(),
                change.desired(),
            );
        }
        if let Some(change) = &self.slowmode_seconds {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "slowmode_seconds",
                change.current(),
                change.desired(),
            );
        }
        if let Some(change) = &self.default_auto_archive_minutes {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "default_auto_archive_minutes",
                display_nullable(change.current().as_ref()),
                display_nullable(change.desired()),
            );
        }
        if let Some(change) = &self.default_thread_slowmode_seconds {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "default_thread_slowmode_seconds",
                change.current(),
                change.desired(),
            );
        }
        for (target, permissions) in &self.overwrites {
            for (permission, change) in permissions {
                render_change_line(
                    output,
                    logical_id,
                    discord_id,
                    &format!("overwrites.{target}.{permission}"),
                    display_overwrite_value(change.current()),
                    display_overwrite_value(change.desired()),
                );
            }
        }
        if self.synced_overwrites.is_some() {
            render_change_line(output, logical_id, discord_id, "permissions_sync", "false", "true");
        }
    }
}

fn nullable_update<T: Clone>(change: Option<&NullableValueChange<T>>) -> ChannelUpdateValue<T> {
    let Some(change) = change else {
        return ChannelUpdateValue::Keep;
    };
    match change {
        NullableValueChange::Set { desired, .. } => ChannelUpdateValue::Set(desired.clone()),
        NullableValueChange::Clear { .. } => ChannelUpdateValue::Clear,
    }
}

fn display_nullable<T: std::fmt::Display>(value: Option<&T>) -> String {
    value.map_or_else(|| "None".to_owned(), ToString::to_string)
}

fn display_nullable_string(value: Option<&String>) -> String {
    value.map_or_else(
        || "None".to_owned(),
        |value| format!("Some({})", display_quoted_string(value)),
    )
}

fn display_overwrite_value(value: &OverwriteValue) -> &'static str {
    match value {
        OverwriteValue::Allow => "allow",
        OverwriteValue::Deny => "deny",
        OverwriteValue::Clear => "clear",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Create,
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
    pub(crate) fn attributes(&self) -> Option<&AttributeChanges> {
        match self {
            Self::Update { attributes, .. } => Some(attributes),
            Self::Create | Self::Release { .. } | Self::Delete { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    changes: BTreeMap<ChannelLogicalId, Change>,
    create_desired: BTreeMap<ChannelLogicalId, CreateDesired>,
    order: Option<OrderPlan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OrderGroupPlan {
    requested: Vec<ChannelLogicalId>,
    updates: Vec<ChannelPositionUpdate>,
    expected_order: Vec<ChannelId>,
}

impl OrderGroupPlan {
    #[cfg(test)]
    pub(crate) fn updates(&self) -> &[ChannelPositionUpdate] {
        &self.updates
    }

    #[cfg(test)]
    pub(crate) fn expected_order(&self) -> &[ChannelId] {
        &self.expected_order
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OrderPlan {
    groups: Vec<OrderGroupPlan>,
}

impl OrderPlan {
    #[cfg(test)]
    pub(crate) fn groups(&self) -> &[OrderGroupPlan] {
        &self.groups
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CreateDesired {
    payload: ChannelCreate,
    parent_logical_id: Option<ChannelLogicalId>,
}

impl CreateDesired {
    /// Plan 時に解決できなかった親だけを、直前の作成結果を含む state から補完します。
    /// その他の create payload は plan 時の concrete 値をそのまま使います。
    fn payload_for_apply(
        &self,
        state: &StateFile,
        catalog: &ChannelCatalog,
        logical_id: &ChannelLogicalId,
    ) -> Result<ChannelCreate, ManagementError> {
        let mut payload = self.payload.clone();
        if let Some(parent_logical_id) = &self.parent_logical_id {
            if payload.parent_id.is_none() {
                let parent_id = state.channels.get(parent_logical_id).copied().ok_or_else(|| {
                    ManagementError::InvalidState(format!(
                        "Channel {logical_id} の親 {parent_logical_id} の作成結果が state にありません"
                    ))
                })?;
                payload.parent_id = Some(parent_id);
            }
            let parent_id = payload
                .parent_id
                .expect("親論理 ID がある create payload は親IDを持ちます");
            if !catalog.channels.iter().any(|channel| channel.id == parent_id) {
                return Err(ManagementError::InvalidState(format!(
                    "Channel {logical_id} の親 {parent_logical_id} の Snowflake {parent_id} が Guild から予期せず消失しています"
                )));
            }
        }
        Ok(payload)
    }
}

pub(crate) type ChannelPlan = Plan;

impl Plan {
    pub(crate) fn len(&self) -> usize {
        self.changes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.order.is_none()
    }

    #[cfg(test)]
    pub(crate) fn order(&self) -> Option<&OrderPlan> {
        self.order.as_ref()
    }

    pub(super) fn has_order(&self) -> bool {
        self.order.is_some()
    }

    pub(super) fn take_order(&mut self) -> Option<OrderPlan> {
        self.order.take()
    }

    pub(super) fn set_order(&mut self, order: Option<OrderPlan>) {
        self.order = order;
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

    fn insert_create(&mut self, logical_id: ChannelLogicalId, change: Change, desired: CreateDesired) {
        debug_assert!(matches!(change, Change::Create));
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
                Change::Create => {
                    output.push_str(&format!("- 新規作成: {logical_id}\n"));
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
        if let Some(order) = &self.order {
            for group in &order.groups {
                output.push_str("- Channel の相対順序: ");
                output.push_str(
                    &group
                        .requested
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" -> "),
                );
                output.push('\n');
            }
        }
        output
    }
}

fn render_create_attributes(desired: &CreateDesired, output: &mut String) {
    let payload = &desired.payload;
    output.push_str("  desired:\n");
    output.push_str(&format!("    type: {}\n", payload.kind.as_str()));
    output.push_str(&format!("    name: {}\n", display_quoted_string(&payload.name)));
    let parent = match (desired.parent_logical_id.as_ref(), payload.parent_id) {
        (Some(logical_id), Some(discord_id)) => format!("{logical_id} ({discord_id})"),
        (Some(logical_id), None) => logical_id.to_string(),
        (None, Some(discord_id)) => discord_id.to_string(),
        (None, None) => "None".to_owned(),
    };
    if payload.kind == ChannelKind::Text {
        output.push_str(&format!("    parent: {parent}\n"));
        output.push_str(&format!(
            "    topic: {}\n",
            display_nullable_string(payload.topic.as_ref())
        ));
        output.push_str(&format!("    nsfw: {}\n", payload.nsfw));
        output.push_str(&format!("    slowmode_seconds: {}\n", payload.slowmode_seconds));
        output.push_str(&format!(
            "    default_auto_archive_minutes: {}\n",
            display_nullable(payload.default_auto_archive_minutes.as_ref())
        ));
        output.push_str(&format!(
            "    default_thread_slowmode_seconds: {}\n",
            payload.default_thread_slowmode_seconds
        ));
    }
    let overwrites = payload
        .overwrites
        .iter()
        .filter_map(|(subject, permissions)| {
            let permissions = permissions
                .known
                .iter()
                .map(|(permission, value)| format!("{permission}: {}", display_overwrite_value(value)))
                .collect::<Vec<_>>();
            (!permissions.is_empty()).then_some(format!("{subject}: {{{}}}", permissions.join(", ")))
        })
        .collect::<Vec<_>>();
    if !overwrites.is_empty() {
        output.push_str(&format!("    overwrites: {{{}}}\n", overwrites.join(", ")));
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
        let Some(discord_id) = state.channels.get(logical_id).copied() else {
            let payload = desired_channel_create_with_catalog(desired, logical_id, state, definition, Some(catalog))?;
            if !can_manage_roles && !payload.overwrites.is_empty() {
                return Err(ManagementError::ChannelPermissionDenied(
                    "Channel の権限上書きには Bot の MANAGE_ROLES 権限が必要です".to_owned(),
                ));
            }
            let parent_logical_id = attributes.parent.as_ref().and_then(ChannelValue::as_value).cloned();
            plan.insert_create(
                logical_id.clone(),
                Change::Create,
                CreateDesired {
                    payload,
                    parent_logical_id,
                },
            );
            continue;
        };
        let Some(current) = actual.get(&discord_id).copied() else {
            return Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
            )));
        };
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
        let synced_overwrites = desired_synced_overwrites(logical_id, &attributes, definition, state, &actual)?;
        if let Some(attributes) = AttributeChanges::between(
            logical_id,
            current,
            &attributes,
            state,
            &planned_channel_creations,
            can_manage_roles,
            synced_overwrites,
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

    plan.order = build_order_plan(definition, state, catalog)?;

    Ok(plan)
}

fn build_order_plan(
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &ChannelCatalog,
) -> Result<Option<OrderPlan>, ManagementError> {
    let Some(order) = definition.order.as_ref() else {
        return Ok(None);
    };
    // 属性更新で親が変わる Channel は、同じ apply の後段で目的の sibling 集合へ
    // 入るため、位置計画も予定された親を投影した catalog を基準に組み立てます。
    let projected_catalog = project_channel_parents_for_order(definition, state, catalog);
    let catalog = &projected_catalog;
    let mut groups = Vec::new();
    if !order.categories.is_empty()
        && let Some(group) = build_order_group(
            &order.categories,
            |channel| channel.parent_id.is_none(),
            ChannelKind::Category,
            definition,
            state,
            catalog,
        )?
    {
        groups.push(group);
    }
    for (parent_logical_id, requested) in &order.children {
        if requested.is_empty() {
            continue;
        }
        let Some(parent_id) = resolve_channel_order_id(parent_logical_id, definition, state, catalog)? else {
            // 親 Category が同じ apply の前段で作成される場合は、実 ID が確定
            // するまで子 Channel の sibling 集合を解決できません。ただし、
            // 参照専用の子は親を変更できないため、作成後に解決できない指定を
            // 保留せず、この時点で診断します。
            validate_deferred_child_order(parent_logical_id, requested, definition, state, catalog)?;
            groups.push(OrderGroupPlan {
                requested: requested.clone(),
                updates: Vec::new(),
                expected_order: Vec::new(),
            });
            continue;
        };
        if let Some(group) = build_order_group(
            requested,
            |channel| channel.parent_id == Some(parent_id),
            ChannelKind::Text,
            definition,
            state,
            catalog,
        )? {
            groups.push(group);
        }
    }
    Ok((!groups.is_empty()).then_some(OrderPlan { groups }))
}

fn validate_deferred_child_order(
    parent_logical_id: &ChannelLogicalId,
    requested_logical_ids: &[ChannelLogicalId],
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &ChannelCatalog,
) -> Result<(), ManagementError> {
    for logical_id in requested_logical_ids {
        let channel_definition = definition
            .channels
            .get(logical_id)
            .expect("order.children の子は DefinitionFile の検証済み宣言だけを参照します");
        let Some(discord_id) = state.channels.get(logical_id).copied() else {
            if channel_definition.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "order.children の参照専用 Channel {logical_id} の対応がありません"
                )));
            }
            continue;
        };
        let Some(channel) = catalog.channels.iter().find(|channel| channel.id == discord_id) else {
            return Err(ManagementError::InvalidState(format!(
                "order.children の Channel {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
            )));
        };
        if channel.kind != ChannelKind::Text {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.children の Channel {logical_id} は Text の兄弟として指定できません"
            )));
        }
        if channel_definition.is_reference() {
            return Err(ManagementError::InvalidDefinition(format!(
                "order.children の未作成 Category {parent_logical_id} には、現在の親が {:?} の参照専用 Channel {logical_id} を配置できません",
                channel.parent_id
            )));
        }
        if !channel.manageable {
            return Err(ManagementError::InvalidState(format!(
                "order.children の Channel {logical_id} の Snowflake {discord_id} は Bot が管理できません"
            )));
        }
    }
    Ok(())
}

fn project_channel_parents_for_order(
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &ChannelCatalog,
) -> ChannelCatalog {
    let mut projected = catalog.clone();
    for (logical_id, channel_definition) in &definition.channels {
        let Some(discord_id) = state.channels.get(logical_id).copied() else {
            continue;
        };
        let Some(parent) = channel_definition.attributes().parent.as_ref() else {
            continue;
        };
        let desired_parent = if let Some(parent_logical_id) = parent.as_value() {
            let Some(parent_id) = state.channels.get(parent_logical_id).copied() else {
                continue;
            };
            Some(parent_id)
        } else if parent.is_clear() {
            None
        } else {
            continue;
        };
        if let Some(channel) = projected.channels.iter_mut().find(|channel| channel.id == discord_id) {
            channel.parent_id = desired_parent;
        }
    }
    projected
}

fn build_order_group(
    requested_logical_ids: &[ChannelLogicalId],
    belongs_to_group: impl Fn(&ChannelSnapshot) -> bool,
    expected_kind: ChannelKind,
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &ChannelCatalog,
) -> Result<Option<OrderGroupPlan>, ManagementError> {
    let actual = catalog
        .channels
        .iter()
        .filter(|channel| belongs_to_group(channel))
        .map(|channel| (channel.id, channel))
        .collect::<BTreeMap<_, _>>();
    // catalog の重複 ID も捨てずに stable_relative_order へ渡し、API 呼出し前に
    // 不正な sibling 一覧として診断できるようにします。
    let mut current = catalog
        .channels
        .iter()
        .filter(|channel| belongs_to_group(channel))
        .map(|channel| channel.id)
        .collect::<Vec<_>>();
    current
        .sort_by(|left, right| compare_position_then_id(&actual[left].position, &actual[right].position, left, right));

    let mut requested = Vec::with_capacity(requested_logical_ids.len());
    let mut fixed = BTreeSet::new();
    let mut deferred = false;
    let mut projected_missing = Vec::new();
    let mut next_placeholder = u64::MAX - 1;
    for logical_id in requested_logical_ids {
        let channel_definition = definition
            .channels
            .get(logical_id)
            .expect("order の Channel は DefinitionFile の検証済み宣言だけを参照します");
        let Some(discord_id) = state.channels.get(logical_id).copied() else {
            if channel_definition.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "order の参照専用 Channel {logical_id} の対応がありません"
                )));
            }
            let placeholder = loop {
                let candidate = ChannelId::new(next_placeholder);
                next_placeholder = next_placeholder.checked_sub(1).ok_or_else(|| {
                    ManagementError::InvalidDefinition(
                        "order の未作成 Channel を投影する ID を確保できません".to_owned(),
                    )
                })?;
                if !catalog.channels.iter().any(|channel| channel.id == candidate)
                    && !projected_missing.contains(&candidate)
                {
                    break candidate;
                }
            };
            projected_missing.push(placeholder);
            deferred = true;
            requested.push(placeholder);
            continue;
        };
        let Some(channel) = actual.get(&discord_id).copied() else {
            return Err(ManagementError::InvalidState(format!(
                "order の Channel {logical_id} の Snowflake {discord_id} が対象の兄弟一覧にありません"
            )));
        };
        if channel.kind != expected_kind {
            return Err(ManagementError::InvalidDefinition(format!(
                "order の Channel {logical_id} は {} の兄弟として指定できません",
                expected_kind.as_str()
            )));
        }
        if channel_definition.is_managed() && !channel.manageable {
            return Err(ManagementError::InvalidState(format!(
                "order の Channel {logical_id} の Snowflake {discord_id} は Bot が管理できません"
            )));
        }
        if channel_definition.is_reference() {
            fixed.insert(discord_id);
        }
        requested.push(discord_id);
    }

    for (logical_id, channel_definition) in &definition.channels {
        let Some(discord_id) = state.channels.get(logical_id).copied() else {
            continue;
        };
        if channel_definition.is_reference() && actual.contains_key(&discord_id) {
            fixed.insert(discord_id);
        }
    }

    if deferred {
        // Discord の新規 Channel は対象 sibling の末尾へ作成されるため、その位置へ
        // 未作成 Channel を投影してから参照専用 anchor との循環を診断します。
        let mut projected_current = current.clone();
        projected_current.extend(projected_missing);
        stable_relative_order(&projected_current, &requested, &fixed).map_err(|()| {
            ManagementError::InvalidDefinition("order の Channel は参照専用対象の固定位置と両立しません".to_owned())
        })?;
        return Ok(Some(OrderGroupPlan {
            requested: requested_logical_ids.to_vec(),
            updates: Vec::new(),
            expected_order: Vec::new(),
        }));
    }

    let desired = stable_relative_order(&current, &requested, &fixed).map_err(|()| {
        ManagementError::InvalidDefinition("order の Channel は参照専用対象の固定位置と両立しません".to_owned())
    })?;
    if desired == current {
        return Ok(None);
    }

    let updates = desired
        .iter()
        .copied()
        .enumerate()
        .map(|(position, channel_id)| ChannelPositionUpdate {
            channel_id,
            position: position as u64,
        })
        .collect();
    Ok(Some(OrderGroupPlan {
        requested: requested_logical_ids.to_vec(),
        updates,
        expected_order: desired,
    }))
}

fn resolve_channel_order_id(
    logical_id: &ChannelLogicalId,
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &ChannelCatalog,
) -> Result<Option<ChannelId>, ManagementError> {
    let channel_definition = definition
        .channels
        .get(logical_id)
        .expect("order.children の親は DefinitionFile の検証済み宣言だけを参照します");
    let Some(discord_id) = state.channels.get(logical_id).copied() else {
        if channel_definition.is_reference() {
            return Err(ManagementError::InvalidState(format!(
                "order.children の参照専用親 Channel {logical_id} の対応がありません"
            )));
        }
        return Ok(None);
    };
    let Some(channel) = catalog.channels.iter().find(|channel| channel.id == discord_id) else {
        return Err(ManagementError::InvalidState(format!(
            "order の親 Channel {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
        )));
    };
    if channel.kind != ChannelKind::Category {
        return Err(ManagementError::InvalidDefinition(format!(
            "order.children の親 Channel {logical_id} は Category ではありません"
        )));
    }
    Ok(Some(discord_id))
}

fn desired_synced_overwrites(
    logical_id: &ChannelLogicalId,
    attributes: &ChannelAttributes,
    definition: &DefinitionFile,
    state: &StateFile,
    actual: &BTreeMap<ChannelId, &ChannelSnapshot>,
) -> Result<Option<BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>>, ManagementError> {
    if attributes.permissions_sync != Some(true) {
        return Ok(None);
    }
    let Some(parent_logical_id) = attributes.parent.as_ref().and_then(ChannelValue::as_value) else {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の permissions_sync には Category の parent が必要です"
        )));
    };
    let mut visiting = BTreeSet::new();
    let overwrites = desired_overwrites_for_channel(parent_logical_id, definition, state, Some(actual), &mut visiting)?;
    Ok(Some(overwrites))
}

fn desired_overwrites_for_channel(
    logical_id: &ChannelLogicalId,
    definition: &DefinitionFile,
    state: &StateFile,
    actual: Option<&BTreeMap<ChannelId, &ChannelSnapshot>>,
    visiting: &mut BTreeSet<ChannelLogicalId>,
) -> Result<BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>, ManagementError> {
    if !visiting.insert(logical_id.clone()) {
        return Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の permissions_sync に循環する親があります"
        )));
    }
    let result = (|| {
        let channel = definition.channels.get(logical_id).ok_or_else(|| {
            ManagementError::InvalidDefinition(format!("Channel {logical_id} の同期先 Category の宣言がありません"))
        })?;
        if channel.is_absent() {
            return Err(ManagementError::InvalidDefinition(format!(
                "Channel {logical_id} の同期先 Category は削除宣言です"
            )));
        }
        let attributes = compose_attributes(channel);
        if attributes.permissions_sync == Some(true) {
            let parent = attributes
                .parent
                .as_ref()
                .and_then(ChannelValue::as_value)
                .ok_or_else(|| {
                    ManagementError::InvalidDefinition(format!(
                        "Channel {logical_id} の permissions_sync には Category の parent が必要です"
                    ))
                })?;
            return desired_overwrites_for_channel(parent, definition, state, actual, visiting);
        }
        let mut overwrites = state
            .channels
            .get(logical_id)
            .and_then(|discord_id| actual.and_then(|catalog| catalog.get(discord_id)))
            .map(|channel| channel.overwrites.clone())
            .unwrap_or_default();
        apply_overwrite_values(logical_id, &attributes, state, &mut overwrites)?;
        Ok(overwrites)
    })();
    visiting.remove(logical_id);
    result
}

fn apply_overwrite_values(
    logical_id: &ChannelLogicalId,
    attributes: &ChannelAttributes,
    state: &StateFile,
    overwrites: &mut BTreeMap<ChannelOverwriteTarget, ChannelOverwritePermissions>,
) -> Result<(), ManagementError> {
    for (subject, permissions) in &attributes.overwrites {
        let target = resolve_overwrite_target(subject, logical_id, state)?;
        let target_permissions = overwrites.entry(target.clone()).or_default();
        for (permission, value) in permissions {
            match value {
                OverwriteValue::Clear => {
                    target_permissions.known.remove(permission);
                }
                value => {
                    target_permissions.known.insert(permission.clone(), *value);
                }
            }
        }
        if target_permissions.known.is_empty()
            && target_permissions.allow_unknown.is_empty()
            && target_permissions.deny_unknown.is_empty()
        {
            overwrites.remove(&target);
        }
    }
    Ok(())
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
            (!state.channels.contains_key(logical_id)).then_some(logical_id.clone())
        })
        .collect()
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
    let current = actual.get(&discord_id).copied();
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
        if attributes.parent.is_none()
            || attributes.parent.as_ref().is_some_and(ChannelValue::is_default)
            || attributes
                .parent
                .as_ref()
                .and_then(ChannelValue::as_value)
                .is_some_and(|parent| parent == category_logical_id)
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
    let Some(parent_logical_id) = parent.as_value() else {
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
    let Some(_) = attributes.name.as_ref().and_then(ChannelValue::as_value) else {
        return Err(ManagementError::InvalidDefinition(format!(
            "新しい Channel {logical_id} には name の具体値が必要です"
        )));
    };
    validate_channel_parent(
        logical_id,
        kind,
        attributes.parent.as_ref(),
        definition,
        state,
        actual,
        planned_channel_creations,
    )?;
    if let Some(parent) = attributes.parent.as_ref().and_then(ChannelValue::as_value)
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
    value
        .as_value()
        .cloned()
        .ok_or_else(|| ManagementError::InvalidDefinition(format!("Channel {logical_id} の name は空にできません")))
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
    if let Some(parent) = value.as_value() {
        if planned_channel_creations.contains(parent) {
            Ok(ParentTarget::Planned(parent.clone()))
        } else if let Some(parent_id) = state.channels.get(parent).copied() {
            Ok(ParentTarget::Resolved(Some(parent_id)))
        } else {
            Err(ManagementError::InvalidState(format!(
                "Channel {logical_id} の親 {parent} の対応がありません"
            )))
        }
    } else if value.is_clear() {
        Ok(ParentTarget::Resolved(None))
    } else {
        Err(ManagementError::InvalidDefinition(format!(
            "Channel {logical_id} の parent に default は指定できません"
        )))
    }
}

fn resolve_topic(value: &ChannelValue<String>) -> Option<String> {
    value.resolve_optional(None)
}

fn resolve_bool(value: &ChannelValue<bool>, default: bool) -> Result<bool, ManagementError> {
    if value.is_clear() {
        return Err(ManagementError::InvalidDefinition(
            "この属性は解除できません".to_owned(),
        ));
    }
    Ok(value.resolve(default, default))
}

fn resolve_u16(value: &ChannelValue<u16>, default: u16) -> Result<u16, ManagementError> {
    Ok(value.resolve(default, 0))
}

fn resolve_nullable_u16(value: &ChannelValue<u16>, default: Option<u16>) -> Result<Option<u16>, ManagementError> {
    Ok(value.resolve_optional(default))
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
    subject: &OverwriteTarget,
    logical_id: &ChannelLogicalId,
    state: &StateFile,
) -> Result<ChannelOverwriteTarget, ManagementError> {
    match subject {
        OverwriteTarget::Everyone => Ok(ChannelOverwriteTarget::Everyone),
        OverwriteTarget::Role(role) => {
            let discord_id = if *role == super::super::configuration::everyone_logical_id() {
                RoleId::new(state.guild_id.get())
            } else {
                state.roles.get(role).copied().ok_or_else(|| {
                    ManagementError::InvalidState(format!(
                        "Channel {logical_id} の権限対象 Role {role} の対応がありません"
                    ))
                })?
            };
            Ok(ChannelOverwriteTarget::Role(discord_id))
        }
        OverwriteTarget::Member(member) => {
            let discord_id = state.members.get(member).copied().ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Channel {logical_id} の権限対象 Member {member} の対応がありません"
                ))
            })?;
            Ok(ChannelOverwriteTarget::Member(discord_id))
        }
    }
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
    let parent_id = attributes
        .parent
        .as_ref()
        .map(|value| resolve_parent_target(value, logical_id, state, &planned_channel_creations))
        .transpose()?
        .and_then(|target| match target {
            ParentTarget::Resolved(parent_id) => parent_id,
            // 同じ plan 内で先に作成する Category は、作成後に state へ追加された
            // snowflake を apply 時にもう一度解決します。
            ParentTarget::Planned(_) => None,
        });
    let topic = attributes.topic.as_ref().and_then(resolve_topic);
    let nsfw = attributes
        .nsfw
        .as_ref()
        .map(|value| resolve_bool(value, DEFAULT_CHANNEL_NSFW))
        .transpose()?
        .unwrap_or(DEFAULT_CHANNEL_NSFW);
    let slowmode_seconds = attributes
        .slowmode_seconds
        .as_ref()
        .map(|value| resolve_u16(value, DEFAULT_SLOWMODE_SECONDS))
        .transpose()?
        .unwrap_or(DEFAULT_SLOWMODE_SECONDS);
    let default_auto_archive_minutes = attributes
        .default_auto_archive_minutes
        .as_ref()
        .map(|value| resolve_nullable_u16(value, DEFAULT_AUTO_ARCHIVE_MINUTES))
        .transpose()?
        .unwrap_or(DEFAULT_AUTO_ARCHIVE_MINUTES);
    let default_thread_slowmode_seconds = attributes
        .default_thread_slowmode_seconds
        .as_ref()
        .map(|value| resolve_u16(value, DEFAULT_THREAD_SLOWMODE_SECONDS))
        .transpose()?
        .unwrap_or(DEFAULT_THREAD_SLOWMODE_SECONDS);
    let overwrites = if attributes.permissions_sync == Some(true) {
        let mut visiting = BTreeSet::new();
        desired_overwrites_for_channel(logical_id, definition_file, state, actual.as_ref(), &mut visiting)?
    } else {
        let mut overwrites = BTreeMap::new();
        apply_overwrite_values(logical_id, &attributes, state, &mut overwrites)?;
        overwrites
    };
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
