use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    configuration::{
        Color, DefinitionFile, KnownPermission, OptionalManagedValueExt, RoleAttributes, RoleDefinition, StateFile,
        everyone_logical_id, resolve_role_id,
    },
    domain::ManagementError,
    port::{RoleCatalog, RoleCreate, RolePositionUpdate, RoleSnapshot, RoleUpdate},
};
use crate::features::discord_management::ids::{RoleId, RoleLogicalId, RoleSettingsSetId};

pub(crate) mod apply;

use super::{display_quoted_string, render_change_line, stable_relative_order};

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
    order: Option<OrderPlan>,
}

pub(crate) type RolePlan = Plan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OrderPlan {
    requested: Vec<RoleLogicalId>,
    updates: Vec<RolePositionUpdate>,
    expected_order: Vec<RoleId>,
}

impl OrderPlan {
    #[cfg(test)]
    pub(crate) fn updates(&self) -> &[RolePositionUpdate] {
        &self.updates
    }

    #[cfg(test)]
    pub(crate) fn expected_order(&self) -> &[RoleId] {
        &self.expected_order
    }
}

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
        if let Some(order) = &self.order {
            output.push_str("- Role の相対順序: ");
            output.push_str(
                &order
                    .requested
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> "),
            );
            output.push('\n');
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
            // @everyone は通常 Role の階層管理対象外ですが、基底権限の更新は
            // 専用 endpoint で許可されるため、snapshot の manageable 判定から
            // 独立して扱います。
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

    plan.order = build_order_plan(definition, state, catalog)?;

    Ok(plan)
}

fn build_order_plan(
    definition: &DefinitionFile,
    state: &StateFile,
    catalog: &RoleCatalog,
) -> Result<Option<OrderPlan>, ManagementError> {
    let Some(order) = definition.order.as_ref() else {
        return Ok(None);
    };
    if order.roles.is_empty() {
        return Ok(None);
    }

    let actual = catalog
        .roles
        .iter()
        .map(|role| (role.id, role))
        .collect::<BTreeMap<_, _>>();
    let mut current = catalog.roles.iter().map(|role| role.id).collect::<Vec<_>>();
    current.sort_by(|left, right| {
        let left = actual[left];
        let right = actual[right];
        let everyone_id = RoleId::new(state.guild_id.get());
        match (left.id == everyone_id, right.id == everyone_id) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => right
                .position
                .cmp(&left.position)
                .then_with(|| left.id.cmp(&right.id)),
        }
    });

    let mut requested = Vec::with_capacity(order.roles.len());
    let mut fixed = BTreeSet::new();
    fixed.extend(
        current
            .iter()
            .copied()
            .filter(|discord_id| !actual[discord_id].manageable),
    );
    // @everyone は Discord が特別扱いする固定 anchor です。fake/adapter の
    // manageable 判定に依存せず、定義に列挙されない場合も直接移動対象に
    // しないで、実在する Guild の末尾位置を保ちます。
    let everyone_id = RoleId::new(state.guild_id.get());
    if actual.contains_key(&everyone_id) {
        fixed.insert(everyone_id);
    }
    let mut deferred = false;
    for logical_id in &order.roles {
        let definition = definition
            .roles
            .get(logical_id)
            .expect("order.roles は DefinitionFile の検証済み宣言だけを参照します");
        let discord_id = if *logical_id == everyone_logical_id() {
            RoleId::new(state.guild_id.get())
        } else if let Some(discord_id) = state.roles.get(logical_id).copied() {
            discord_id
        } else {
            if definition.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "order.roles の参照専用 Role {logical_id} の対応がありません"
                )));
            }
            deferred = true;
            continue;
        };
        let Some(role) = actual.get(&discord_id).copied() else {
            return Err(ManagementError::InvalidState(format!(
                "order.roles の Role {logical_id} の Snowflake {discord_id} が Guild から予期せず消失しています"
            )));
        };
        if definition.is_managed()
            && *logical_id != everyone_logical_id()
            && !role.manageable
        {
            return Err(ManagementError::InvalidState(format!(
                "order.roles の Role {logical_id} は Bot の階層より上位または管理対象外のため移動できません"
            )));
        }
        if definition.is_reference()
            || *logical_id == everyone_logical_id()
            || !role.manageable
        {
            fixed.insert(discord_id);
        }
        requested.push(discord_id);
    }

    for (logical_id, role) in &definition.roles {
        let Some(discord_id) = (if *logical_id == everyone_logical_id() {
            Some(RoleId::new(state.guild_id.get()))
        } else {
            state.roles.get(logical_id).copied()
        }) else {
            continue;
        };
        if role.is_reference() {
            fixed.insert(discord_id);
        }
    }

    if deferred {
        // 作成前の managed Role 自体の位置はまだ未知でも、既存 Role だけで
        // 参照専用 anchor をまたぐ矛盾は、lifecycle を呼ぶ前に診断できます。
        stable_relative_order(&current, &requested, &fixed).map_err(|()| {
            ManagementError::InvalidDefinition(
                "order.roles は参照専用 Role、@everyone、Role 階層の固定位置と両立しません".to_owned(),
            )
        })?;
        return Ok(Some(OrderPlan {
            requested: order.roles.clone(),
            updates: Vec::new(),
            expected_order: Vec::new(),
        }));
    }

    let desired = stable_relative_order(&current, &requested, &fixed).map_err(|()| {
        ManagementError::InvalidDefinition(
            "order.roles は参照専用 Role、@everyone、Role 階層の固定位置と両立しません".to_owned(),
        )
    })?;
    if desired == current {
        return Ok(None);
    }

    let mut current_indices = BTreeMap::new();
    for (index, discord_id) in current.iter().copied().enumerate() {
        current_indices.insert(discord_id, index);
    }
    let mut desired_indices = BTreeMap::new();
    for (index, discord_id) in desired.iter().copied().enumerate() {
        desired_indices.insert(discord_id, index);
    }
    let mut updates = Vec::new();
    for discord_id in current.iter().copied() {
        if fixed.contains(&discord_id)
            || !requested.contains(&discord_id)
            || current_indices[&discord_id] == desired_indices[&discord_id]
        {
            continue;
        }
        let position = i16::try_from(desired.len() - 1 - desired_indices[&discord_id]).map_err(|_| {
            ManagementError::InvalidDefinition("Role の数が Discord の position 範囲を超えています".to_owned())
        })?;
        updates.push(RolePositionUpdate { role_id: discord_id, position });
    }

    Ok(Some(OrderPlan {
        requested: order.roles.clone(),
        updates,
        expected_order: desired,
    }))
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

pub(super) fn ordered_role_ids(catalog: &RoleCatalog, everyone_id: RoleId) -> Vec<RoleId> {
    let mut roles = catalog.roles.iter().collect::<Vec<_>>();
    roles.sort_by(|left, right| {
        match (left.id == everyone_id, right.id == everyone_id) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => right
                .position
                .cmp(&left.position)
                .then_with(|| left.id.cmp(&right.id)),
        }
    });
    roles.into_iter().map(|role| role.id).collect()
}
