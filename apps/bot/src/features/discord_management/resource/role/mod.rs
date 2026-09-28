use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    configuration::{
        Color, DefinitionFile, KnownPermission, OptionalManagedValueExt, RoleAttributes, RoleDefinition, StateFile,
        everyone_logical_id, resolve_role_id,
    },
    domain::ManagementError,
    port::{RoleCatalog, RoleCreate, RoleSnapshot, RoleUpdate},
};
use crate::features::discord_management::ids::{RoleId, RoleLogicalId, RoleSettingsSetId};

pub(crate) mod apply;

use super::{display_quoted_string, render_change_line};

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
        let name = desired
            .name
            .resolve_optional("new role".to_owned())
            .and_then(|value| ValueChange::between(actual.name.clone(), value));
        let color = desired
            .color
            .resolve_optional(Color::default())
            .and_then(|value| ValueChange::between(actual.color, value));
        let hoist = desired
            .hoist
            .resolve_optional(false)
            .and_then(|value| ValueChange::between(actual.hoist, value));
        let mentionable = desired
            .mentionable
            .resolve_optional(false)
            .and_then(|value| ValueChange::between(actual.mentionable, value));
        let mut permissions = BTreeMap::new();
        for (permission, value) in &desired.permissions {
            let current = *actual
                .permissions
                .get(permission)
                .expect("RoleCatalog は既知の権限をすべての Role に保持します");
            let default = *default_permissions
                .get(permission)
                .expect("RoleCatalog は既知の権限の Guild 既定値をすべて保持します");
            let desired = value.resolve(default);
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
            render_change_line(
                output,
                logical_id,
                discord_id,
                "name",
                display_quoted_string(change.current()),
                display_quoted_string(change.desired()),
            );
        }
        if let Some(change) = &self.color {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "color",
                change.current().get(),
                change.desired().get(),
            );
        }
        if let Some(change) = &self.hoist {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "hoist",
                change.current(),
                change.desired(),
            );
        }
        if let Some(change) = &self.mentionable {
            render_change_line(
                output,
                logical_id,
                discord_id,
                "mentionable",
                change.current(),
                change.desired(),
            );
        }
        for (permission, change) in &self.permissions {
            render_change_line(
                output,
                logical_id,
                discord_id,
                &format!("permissions.{permission}"),
                change.current(),
                change.desired(),
            );
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Create,
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
    pub(crate) fn discord_id(&self) -> Option<RoleId> {
        match self {
            Self::Create => None,
            Self::Update { discord_id, .. } | Self::Release { discord_id } | Self::Delete { discord_id } => {
                Some(*discord_id)
            }
        }
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
    changes: BTreeMap<RoleLogicalId, Change>,
    create_desired: BTreeMap<RoleLogicalId, RoleCreate>,
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

    fn insert_create(&mut self, logical_id: RoleLogicalId, change: Change, desired: RoleCreate) {
        debug_assert!(matches!(change, Change::Create));
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
                Change::Create => {
                    output.push_str(&format!("- 新規作成: {logical_id}\n"));
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

fn render_create_attributes(attributes: &RoleCreate, output: &mut String) {
    output.push_str("  desired:\n");
    output.push_str(&format!("    name: {}\n", display_quoted_string(&attributes.name)));
    output.push_str(&format!("    color: {}\n", attributes.color.get()));
    output.push_str(&format!("    hoist: {}\n", attributes.hoist));
    output.push_str(&format!("    mentionable: {}\n", attributes.mentionable));
    if !attributes.permissions.is_empty() {
        output.push_str("    permissions: {");
        for (index, (permission, value)) in attributes.permissions.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            output.push_str(&format!("{permission}: {value}"));
        }
        output.push_str("}\n");
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

/// Role 作成時に Discord API へ送る concrete payload を組み立てます。
///
/// 作成 plan の表示と apply の payload が同じ既定値を使うため、plan にこの payload を
/// 保持します。入力は parse/validator 済みですが、設定セット合成後にしか決められない
/// 作成時の必須値と Discord 側 capability はここで検査します。
fn build_role_create(
    desired: &RoleAttributes,
    permission_names: &BTreeSet<KnownPermission>,
    default_permissions: &BTreeMap<KnownPermission, bool>,
    grantable_permissions: &BTreeSet<KnownPermission>,
    logical_id: &RoleLogicalId,
) -> Result<RoleCreate, ManagementError> {
    let name = desired
        .name
        .resolve_optional("new role".to_owned())
        .ok_or_else(|| ManagementError::InvalidDefinition(format!("新しい Role {logical_id} には name が必要です")))?;

    let mut permissions: BTreeMap<_, _> = permission_names
        .iter()
        .map(|permission| {
            let default = *default_permissions
                .get(permission)
                .expect("RoleCatalog は既知の権限の Guild 既定値をすべて保持します");
            (permission.clone(), default)
        })
        .collect();
    for (permission, value) in &desired.permissions {
        let default = *default_permissions
            .get(permission)
            .expect("RoleCatalog は既知の権限の Guild 既定値をすべて保持します");
        let resolved = value.resolve(default);
        if resolved && !grantable_permissions.contains(permission) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に権限 {permission} を付与できません。Bot 自身がこの権限を持っていません"
            )));
        }
        permissions.insert(permission.clone(), resolved);
    }

    Ok(RoleCreate {
        name,
        color: desired.color.resolve_optional(Color::default()).unwrap_or_default(),
        hoist: desired.hoist.resolve_optional(false).unwrap_or(false),
        mentionable: desired.mentionable.resolve_optional(false).unwrap_or(false),
        permissions,
    })
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
            add_update(&mut plan, logical_id, discord_id, actual, &desired_attributes, catalog)?;
            continue;
        }

        let Some(discord_id) = state.roles.get(logical_id).copied() else {
            if desired.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "参照専用 Role {logical_id} の対応がありません"
                )));
            }
            let attributes = compose_attributes(desired, &definition.settings_sets.role);
            let create = build_role_create(
                &attributes,
                &catalog.permission_names,
                &catalog.default_permissions,
                &catalog.grantable_permissions,
                logical_id,
            )?;
            plan.insert_create(logical_id.clone(), Change::Create, create);
            continue;
        };

        let Some(actual) = actual_roles.get(&discord_id) else {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
            )));
        };
        let desired_attributes = compose_attributes(desired, &definition.settings_sets.role);
        if desired.is_managed() && !actual.manageable {
            return Err(ManagementError::InvalidState(format!(
                "Role {logical_id} の Snowflake {discord_id} は Bot が管理できません"
            )));
        }
        add_update(&mut plan, logical_id, discord_id, actual, &desired_attributes, catalog)?;
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

fn add_update(
    plan: &mut Plan,
    logical_id: &RoleLogicalId,
    discord_id: RoleId,
    actual: &RoleSnapshot,
    desired: &RoleAttributes,
    catalog: &RoleCatalog,
) -> Result<(), ManagementError> {
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
