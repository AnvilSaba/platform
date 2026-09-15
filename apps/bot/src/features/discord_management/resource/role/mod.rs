use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    configuration::{
        Color, DefinitionFile, KnownPermission, ManagedValue, RoleAttributes, RoleDefinition, StateFile,
        everyone_logical_id, resolve_role_id,
    },
    domain::ManagementError,
    port::{RoleCatalog, RoleSnapshot, RoleUpdate},
};
use crate::features::discord_management::ids::{RoleId, RoleLogicalId, RoleSettingsSetId};

pub(crate) mod apply;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttributeChanges {
    intent_fingerprint: String,
    name: Option<ValueChange<String>>,
    color: Option<ValueChange<Color>>,
    hoist: Option<ValueChange<bool>>,
    mentionable: Option<ValueChange<bool>>,
    permissions: BTreeMap<KnownPermission, ValueChange<bool>>,
}

impl AttributeChanges {
    fn between(
        logical_id: &RoleLogicalId,
        actual: &RoleSnapshot,
        desired: &RoleAttributes,
        default_permissions: &BTreeMap<KnownPermission, bool>,
        grantable_permissions: &BTreeSet<KnownPermission>,
    ) -> Result<Option<Self>, ManagementError> {
        let intent_fingerprint = fingerprint(&format!("{desired:?}"));
        let name = desired
            .name
            .as_ref()
            .and_then(|value| ValueChange::between(actual.name.clone(), resolve(value, "new role".to_owned())));
        let color = desired
            .color
            .as_ref()
            .and_then(|value| ValueChange::between(actual.color, resolve(value, Color::default())));
        let hoist = desired
            .hoist
            .as_ref()
            .and_then(|value| ValueChange::between(actual.hoist, resolve(value, false)));
        let mentionable = desired
            .mentionable
            .as_ref()
            .and_then(|value| ValueChange::between(actual.mentionable, resolve(value, false)));
        let mut permissions = BTreeMap::new();
        for (permission, value) in &desired.permissions {
            let current = *actual
                .permissions
                .get(permission)
                .expect("RoleCatalog は既知の権限をすべての Role に保持します");
            let default = *default_permissions
                .get(permission)
                .expect("RoleCatalog は既知の権限の Guild 既定値をすべて保持します");
            let desired = resolve(value, default);
            if !current && desired && !grantable_permissions.contains(permission) {
                return Err(ManagementError::InvalidDefinition(format!(
                    "Role {logical_id} に権限 {permission} を付与できません。Bot 自身がこの権限を持っていません"
                )));
            }
            if let Some(change) = ValueChange::between(current, desired) {
                permissions.insert(permission.clone(), change);
            }
        }

        let changes = Self {
            intent_fingerprint,
            name,
            color,
            hoist,
            mentionable,
            permissions,
        };
        if changes.is_empty() {
            Ok(None)
        } else {
            Ok(Some(changes))
        }
    }

    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.color.is_none()
            && self.hoist.is_none()
            && self.mentionable.is_none()
            && self.permissions.is_empty()
    }

    /// 現在値を除いた更新意図の fingerprint です。
    pub(crate) fn intent_fingerprint(&self) -> String {
        self.intent_fingerprint.clone()
    }

    #[cfg(test)]
    pub(crate) fn name(&self) -> Option<&ValueChange<String>> {
        self.name.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn color(&self) -> Option<&ValueChange<Color>> {
        self.color.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn hoist(&self) -> Option<&ValueChange<bool>> {
        self.hoist.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn mentionable(&self) -> Option<&ValueChange<bool>> {
        self.mentionable.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn permissions(&self) -> &BTreeMap<KnownPermission, ValueChange<bool>> {
        &self.permissions
    }

    fn to_update(&self, actual_permissions: &BTreeMap<KnownPermission, bool>) -> RoleUpdate {
        let permissions = (!self.permissions.is_empty()).then(|| {
            let mut permissions = actual_permissions.clone();
            for (permission, change) in &self.permissions {
                permissions.insert(permission.clone(), *change.desired());
            }
            permissions
        });
        RoleUpdate {
            name: self.name.as_ref().map(|change| change.desired().clone()),
            color: self.color.as_ref().map(|change| *change.desired()),
            hoist: self.hoist.as_ref().map(|change| *change.desired()),
            mentionable: self.mentionable.as_ref().map(|change| *change.desired()),
            permissions,
        }
    }

    fn render(&self, logical_id: &RoleLogicalId, discord_id: &RoleId, output: &mut String) {
        if let Some(change) = &self.name {
            render_value_change(output, logical_id, discord_id, "name", change);
        }
        if let Some(change) = &self.color {
            output.push_str(&format!(
                "- {} ({}) color: {} -> {}\n",
                logical_id,
                discord_id,
                change.current().get(),
                change.desired().get()
            ));
        }
        if let Some(change) = &self.hoist {
            render_value_change(output, logical_id, discord_id, "hoist", change);
        }
        if let Some(change) = &self.mentionable {
            render_value_change(output, logical_id, discord_id, "mentionable", change);
        }
        for (permission, change) in &self.permissions {
            render_value_change(
                output,
                logical_id,
                discord_id,
                &format!("permissions.{permission}"),
                change,
            );
        }
    }
}

fn render_value_change<T: std::fmt::Display>(
    output: &mut String,
    logical_id: &RoleLogicalId,
    discord_id: &RoleId,
    attribute: &str,
    change: &ValueChange<T>,
) {
    output.push_str(&format!(
        "- {} ({}) {}: {} -> {}\n",
        logical_id,
        discord_id,
        attribute,
        change.current(),
        change.desired()
    ));
}

fn fingerprint(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3_u64);
    }
    format!("{hash:016x}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Create {
        recreated: bool,
    },
    Update {
        discord_id: RoleId,
        attributes: AttributeChanges,
    },
    Release {
        discord_id: RoleId,
    },
    Delete {
        discord_id: RoleId,
    },
}

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
    pub(crate) fn discord_id(&self) -> Option<RoleId> {
        match self {
            Self::Create { .. } => None,
            Self::Update { discord_id, .. } | Self::Release { discord_id } | Self::Delete { discord_id } => {
                Some(*discord_id)
            }
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
    changes: BTreeMap<RoleLogicalId, Change>,
    create_desired: BTreeMap<RoleLogicalId, RoleAttributes>,
}

pub(crate) type RolePlan = Plan;

impl Plan {
    pub(crate) fn len(&self) -> usize {
        self.changes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&RoleLogicalId, &Change)> {
        self.changes.iter()
    }

    pub(crate) fn get(&self, logical_id: &RoleLogicalId) -> Option<&Change> {
        self.changes.get(logical_id)
    }

    pub(crate) fn contains_deletions(&self) -> bool {
        self.changes.values().any(Change::is_delete)
    }

    pub(super) fn insert(&mut self, logical_id: RoleLogicalId, change: Change) {
        debug_assert!(self.changes.insert(logical_id, change).is_none());
    }

    fn insert_create(&mut self, logical_id: RoleLogicalId, change: Change, desired: RoleAttributes) {
        debug_assert!(matches!(change, Change::Create { .. }));
        debug_assert!(self.changes.insert(logical_id.clone(), change).is_none());
        self.create_desired.insert(logical_id, desired);
    }

    pub(super) fn remove(&mut self, logical_id: &RoleLogicalId) -> Option<Change> {
        self.create_desired.remove(logical_id);
        self.changes.remove(logical_id)
    }

    pub(crate) fn render(&self) -> String {
        if self.is_empty() {
            return "変更はありません。\n".to_owned();
        }

        let mut output = String::from("Role の変更計画\n\n");
        for (logical_id, change) in &self.changes {
            match change {
                Change::Create { recreated } => {
                    let action = if *recreated { "再作成" } else { "新規作成" };
                    output.push_str(&format!("- {action}: {logical_id}\n"));
                    if let Some(desired) = self.create_desired.get(logical_id) {
                        render_create_attributes(desired, &mut output);
                    }
                }
                Change::Update {
                    discord_id,
                    attributes,
                } => attributes.render(logical_id, discord_id, &mut output),
                Change::Release { discord_id } => {
                    output.push_str(&format!("- 管理解除: {logical_id} ({discord_id})\n"));
                }
                Change::Delete { discord_id } => output.push_str(&format!(
                    "- 削除: {logical_id} ({discord_id})\n  影響: Guild から Role が削除され、付与済みの割り当ても失われます。\n"
                )),
            }
        }
        output
    }
}

fn render_create_attributes(attributes: &RoleAttributes, output: &mut String) {
    output.push_str("  desired:\n");
    if let Some(name) = &attributes.name {
        render_create_value(output, "name", name);
    }
    if let Some(color) = &attributes.color {
        if let ManagedValue::Value(color) = color {
            output.push_str(&format!("    color: {}\n", color.get()));
        }
    }
    if let Some(hoist) = &attributes.hoist {
        render_create_value(output, "hoist", hoist);
    }
    if let Some(mentionable) = &attributes.mentionable {
        render_create_value(output, "mentionable", mentionable);
    }
    if !attributes.permissions.is_empty() {
        output.push_str("    permissions: {");
        for (index, (permission, value)) in attributes.permissions.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            if let ManagedValue::Value(value) = value {
                output.push_str(&format!("{permission}: {value:?}"));
            }
        }
        output.push_str("}\n");
    }
}

fn render_create_value<T: std::fmt::Debug>(output: &mut String, attribute: &str, value: &ManagedValue<T>) {
    if let ManagedValue::Value(value) = value {
        output.push_str(&format!("    {attribute}: {value:?}\n"));
    }
}

pub(crate) fn compose_attributes(
    definition: &RoleDefinition,
    settings_sets: &BTreeMap<RoleSettingsSetId, RoleAttributes>,
) -> RoleAttributes {
    let mut composed = RoleAttributes::default();
    for name in definition.settings_sets() {
        let attributes = settings_sets
            .get(name)
            .expect("検証済み Role 定義は既知の設定セットだけを参照します");
        composed.merge(attributes);
    }
    composed.merge(definition.attributes());
    composed
}

/// Role 作成時に Discord API へ送る具体値へ、明示した属性だけを解決します。
///
/// 作成 plan の表示と apply の payload が同じ既定値を使うための共有 resolver です。
pub(crate) fn resolve_role_create_attributes(
    desired: &RoleAttributes,
    default_permissions: &BTreeMap<KnownPermission, bool>,
) -> RoleAttributes {
    RoleAttributes {
        name: desired
            .name
            .as_ref()
            .map(|value| ManagedValue::Value(resolve(value, "new role".to_owned()))),
        color: desired
            .color
            .as_ref()
            .map(|value| ManagedValue::Value(resolve(value, Color::default()))),
        hoist: desired
            .hoist
            .as_ref()
            .map(|value| ManagedValue::Value(resolve(value, false))),
        mentionable: desired
            .mentionable
            .as_ref()
            .map(|value| ManagedValue::Value(resolve(value, false))),
        permissions: desired
            .permissions
            .iter()
            .map(|(permission, value)| {
                let default = *default_permissions
                    .get(permission)
                    .expect("RoleCatalog は既知の権限の Guild 既定値をすべて保持します");
                (permission.clone(), ManagedValue::Value(resolve(value, default)))
            })
            .collect(),
    }
}

pub(crate) fn resolve<T: Clone>(value: &ManagedValue<T>, default: T) -> T {
    match value {
        ManagedValue::Value(value) => value.clone(),
        ManagedValue::Default => default,
    }
}

/// Role の希望構成と実構成から、Role 部分の変更計画を組み立てます。
///
/// この関数は Discord へアクセスせず、`RoleCatalog` と検証済みの構成だけを
/// 入力に取るため、全体 `plan` の内部シームとしてテストできます。
pub(crate) fn build_plan(
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &RoleCatalog,
) -> Result<Plan, ManagementError> {
    let actual_roles = catalog
        .roles
        .iter()
        .map(|role| (role.id, role))
        .collect::<BTreeMap<_, _>>();
    let mut plan = Plan::default();

    if let Some(logical_id) = state.pending_creations.iter().next() {
        return Err(ManagementError::InvalidState(format!(
            "Role {logical_id} は作成結果不明のため、同じ定義で状態を確認する必要があります"
        )));
    }
    for logical_id in &state.pending_deletions {
        let Some(role) = definition.roles.get(logical_id) else {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の削除意図が未解決のため、定義を変更できません"
            )));
        };
        if !role.is_absent() {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の削除意図が未解決です"
            )));
        }
    }
    for logical_id in state.pending_role_updates.keys() {
        let Some(role) = definition.roles.get(logical_id) else {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の更新意図が未解決のため、定義を変更できません"
            )));
        };
        if !role.is_managed() {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の更新意図が未解決です"
            )));
        }
    }

    for (logical_id, desired) in &definition.roles {
        if desired.is_absent() {
            if *logical_id == everyone_logical_id() {
                return Err(ManagementError::InvalidDefinition(
                    "@everyone Role は削除できません".to_owned(),
                ));
            }
            let Some(discord_id) = state.roles.get(logical_id).copied() else {
                continue;
            };
            if state.deleted_roles.contains(logical_id) {
                continue;
            }
            if !state.pending_deletions.contains(logical_id) && !actual_roles.contains_key(&discord_id) {
                return Err(ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
                )));
            }
            if let Some(actual) = actual_roles.get(&discord_id)
                && !actual.manageable
            {
                return Err(ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {discord_id} は Bot が管理できないため削除できません"
                )));
            }
            plan.insert(logical_id.clone(), Change::Delete { discord_id });
            continue;
        }

        if *logical_id == everyone_logical_id() {
            let discord_id = resolve_role_id(logical_id, state)?;
            let actual = actual_roles.get(&discord_id).ok_or_else(|| {
                ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
                ))
            })?;
            let desired_attributes = compose_attributes(desired, &definition.settings_sets.role);
            if desired_attributes.has_non_permission_attributes() {
                return Err(ManagementError::InvalidDefinition(
                    "@everyone Role では権限だけを管理できます".to_owned(),
                ));
            }
            if desired.is_managed() && !actual.manageable {
                return Err(ManagementError::InvalidState(format!(
                    "Role {logical_id} の Snowflake {discord_id} は Bot が管理できません"
                )));
            }
            add_update(
                &mut plan,
                logical_id,
                discord_id,
                actual,
                &desired_attributes,
                catalog,
                state,
            )?;
            continue;
        }

        if state.deleted_roles.contains(logical_id) {
            if desired.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "削除済みの Role {logical_id} は参照専用として利用できません"
                )));
            }
            validate_role_creation(logical_id, desired, &definition.settings_sets.role, catalog)?;
            let attributes = compose_attributes(desired, &definition.settings_sets.role);
            plan.insert_create(
                logical_id.clone(),
                Change::Create { recreated: true },
                resolve_role_create_attributes(&attributes, &catalog.default_permissions),
            );
            continue;
        }

        let Some(discord_id) = state.roles.get(logical_id).copied() else {
            if desired.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "参照専用 Role {logical_id} の対応がありません"
                )));
            }
            validate_role_creation(logical_id, desired, &definition.settings_sets.role, catalog)?;
            let attributes = compose_attributes(desired, &definition.settings_sets.role);
            plan.insert_create(
                logical_id.clone(),
                Change::Create { recreated: false },
                resolve_role_create_attributes(&attributes, &catalog.default_permissions),
            );
            continue;
        };

        if state.pending_deletions.contains(logical_id) {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の削除意図が未解決です"
            )));
        }

        let actual = actual_roles.get(&discord_id).ok_or_else(|| {
            ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
            ))
        })?;
        let desired_attributes = compose_attributes(desired, &definition.settings_sets.role);
        if desired.is_managed() && !actual.manageable {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} は Bot が管理できません"
            )));
        }
        add_update(
            &mut plan,
            logical_id,
            discord_id,
            actual,
            &desired_attributes,
            catalog,
            state,
        )?;
    }

    for (logical_id, discord_id) in &state.roles {
        if *logical_id == everyone_logical_id() || definition.roles.contains_key(logical_id) {
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

/// 応答不明の更新について、再取得した実構成が希望値へ到達していれば
/// 未完了 marker を解決します。
pub(crate) fn reconcile_pending_updates(
    definition: &DefinitionFile,
    state: &mut StateFile,
    catalog: &RoleCatalog,
) -> Result<(), ManagementError> {
    let actual_roles = catalog
        .roles
        .iter()
        .map(|role| (role.id, role))
        .collect::<BTreeMap<_, _>>();
    let pending_logical_ids = state.pending_role_updates.keys().cloned().collect::<Vec<_>>();
    for logical_id in pending_logical_ids {
        let Some(role_definition) = definition.roles.get(&logical_id) else {
            continue;
        };
        if !role_definition.is_managed() {
            continue;
        }
        let Ok(role_id) = resolve_role_id(&logical_id, state) else {
            continue;
        };
        let Some(actual) = actual_roles.get(&role_id).copied() else {
            continue;
        };
        let desired = compose_attributes(role_definition, &definition.settings_sets.role);
        if AttributeChanges::between(
            &logical_id,
            actual,
            &desired,
            &catalog.default_permissions,
            &catalog.grantable_permissions,
        )?
        .is_none()
        {
            state.pending_role_updates.remove(&logical_id);
        }
    }
    Ok(())
}

fn add_update(
    plan: &mut Plan,
    logical_id: &RoleLogicalId,
    discord_id: RoleId,
    actual: &RoleSnapshot,
    desired: &RoleAttributes,
    catalog: &RoleCatalog,
    state: &StateFile,
) -> Result<(), ManagementError> {
    if let Some(pending) = state.pending_role_updates.get(logical_id)
        && (pending.discord_id != discord_id || pending.fingerprint != fingerprint(&format!("{desired:?}")))
    {
        return Err(ManagementError::InvalidState(format!(
            "Role {logical_id} の未完了更新 intent と今回の定義が一致しません"
        )));
    }
    if let Some(attributes) = AttributeChanges::between(
        logical_id,
        actual,
        desired,
        &catalog.default_permissions,
        &catalog.grantable_permissions,
    )? {
        plan.insert(logical_id.clone(), Change::Update { discord_id, attributes });
    }
    Ok(())
}

fn validate_role_creation(
    logical_id: &RoleLogicalId,
    definition: &RoleDefinition,
    settings_sets: &BTreeMap<RoleSettingsSetId, RoleAttributes>,
    catalog: &RoleCatalog,
) -> Result<(), ManagementError> {
    let attributes = compose_attributes(definition, settings_sets);
    let Some(name) = attributes.name.as_ref() else {
        return Err(ManagementError::InvalidDefinition(format!(
            "新しい Role {logical_id} には name が必要です"
        )));
    };
    let name = resolve(name, "new role".to_owned());
    if name.is_empty() {
        return Err(ManagementError::InvalidDefinition(format!(
            "新しい Role {logical_id} の name は空にできません"
        )));
    }
    for (permission, value) in &attributes.permissions {
        let default = *catalog
            .default_permissions
            .get(permission)
            .expect("RoleCatalog は既知の権限の Guild 既定値をすべて保持します");
        let resolved = resolve(value, default);
        if resolved && !catalog.grantable_permissions.contains(permission) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に権限 {permission} を付与できません。Bot 自身がこの権限を持っていません"
            )));
        }
    }
    Ok(())
}
