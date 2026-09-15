use std::collections::{BTreeMap, BTreeSet};

use super::super::{
    configuration::{
        Color, DefinitionFile, ManagedValue, RoleAttributes, RoleDefinition, StateFile, everyone_logical_id,
        resolve_role_id,
    },
    domain::ManagementError,
    port::{RoleCatalog, RoleSnapshot},
};
use crate::features::discord_management::ids::{RoleId, RoleLogicalId, RoleSettingsSetId};

pub(crate) mod apply;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttributeChange {
    pub logical_id: RoleLogicalId,
    pub discord_id: RoleId,
    pub attribute: String,
    pub current: String,
    pub desired: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RoleLifecycleChange {
    Create {
        logical_id: RoleLogicalId,
        recreated: bool,
    },
    Release {
        logical_id: RoleLogicalId,
        discord_id: RoleId,
    },
    Delete {
        logical_id: RoleLogicalId,
        discord_id: RoleId,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RolePlan {
    pub changes: Vec<AttributeChange>,
    pub lifecycle: Vec<RoleLifecycleChange>,
}

impl RolePlan {
    pub(crate) fn render(&self) -> String {
        if self.changes.is_empty() && self.lifecycle.is_empty() {
            return "変更はありません。\n".to_owned();
        }

        let mut output = String::from("Role の変更計画\n\n");
        for change in &self.lifecycle {
            match change {
                RoleLifecycleChange::Create {
                    logical_id,
                    recreated,
                } => {
                    let action = if *recreated { "再作成" } else { "新規作成" };
                    output.push_str(&format!("- {action}: {logical_id}\n"));
                }
                RoleLifecycleChange::Release {
                    logical_id,
                    discord_id,
                } => output.push_str(&format!("- 管理解除: {logical_id} ({discord_id})\n")),
                RoleLifecycleChange::Delete {
                    logical_id,
                    discord_id,
                } => output.push_str(&format!(
                    "- 削除: {logical_id} ({discord_id})\n  影響: Guild から Role が削除され、付与済みの割り当ても失われます。\n"
                )),
            }
        }
        for change in &self.changes {
            output.push_str(&format!(
                "- {} ({}) {}: {} -> {}\n",
                change.logical_id, change.discord_id, change.attribute, change.current, change.desired
            ));
        }
        output
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

fn validate_permission_names(
    definition: &DefinitionFile,
    known_permissions: &BTreeSet<String>,
) -> Result<(), ManagementError> {
    for (name, attributes) in &definition.settings_sets.role {
        validate_attribute_permission_names(attributes, known_permissions, &format!("Role 設定セット {name}"))?;
    }
    for (logical_id, role) in &definition.roles {
        validate_attribute_permission_names(role.attributes(), known_permissions, &format!("Role {logical_id}"))?;
    }
    Ok(())
}

fn validate_attribute_permission_names(
    attributes: &RoleAttributes,
    known_permissions: &BTreeSet<String>,
    context: &str,
) -> Result<(), ManagementError> {
    if let Some(permission) = attributes
        .permissions
        .keys()
        .find(|permission| !known_permissions.contains(*permission))
    {
        return Err(ManagementError::InvalidDefinition(format!(
            "{context} に未知の権限 {permission} が指定されています"
        )));
    }
    Ok(())
}

fn compare_attributes(
    logical_id: &RoleLogicalId,
    discord_id: &RoleId,
    actual: &RoleSnapshot,
    desired: &RoleAttributes,
    default_permissions: &BTreeMap<String, bool>,
    grantable_permissions: &BTreeSet<String>,
    changes: &mut Vec<AttributeChange>,
) -> Result<(), ManagementError> {
    if let Some(value) = &desired.name {
        let desired = resolve(value, "new role".to_owned());
        push_change(changes, logical_id, discord_id, "name", &actual.name, &desired);
    }
    if let Some(value) = &desired.color {
        let desired = resolve(value, Color::default());
        push_change(
            changes,
            logical_id,
            discord_id,
            "color",
            &actual.color.get().to_string(),
            &desired.get().to_string(),
        );
    }
    if let Some(value) = &desired.hoist {
        let desired = resolve(value, false);
        push_change(
            changes,
            logical_id,
            discord_id,
            "hoist",
            &actual.hoist.to_string(),
            &desired.to_string(),
        );
    }
    if let Some(value) = &desired.mentionable {
        let desired = resolve(value, false);
        push_change(
            changes,
            logical_id,
            discord_id,
            "mentionable",
            &actual.mentionable.to_string(),
            &desired.to_string(),
        );
    }
    for (permission, value) in &desired.permissions {
        let Some(current) = actual.permissions.get(permission) else {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に未知の権限 {permission} が指定されています"
            )));
        };
        let default = *default_permissions
            .get(permission)
            .ok_or_else(|| ManagementError::RoleSource(format!("権限 {permission} の Guild 既定値を取得できません")))?;
        let desired = resolve(value, default);
        if !*current && desired && !grantable_permissions.contains(permission) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に権限 {permission} を付与できません。Bot 自身がこの権限を持っていません"
            )));
        }
        push_change(
            changes,
            logical_id,
            discord_id,
            &format!("permissions.{permission}"),
            &current.to_string(),
            &desired.to_string(),
        );
    }
    Ok(())
}

pub(crate) fn resolve<T: Clone>(value: &ManagedValue<T>, default: T) -> T {
    match value {
        ManagedValue::Value(value) => value.clone(),
        ManagedValue::Default => default,
    }
}

fn push_change(
    changes: &mut Vec<AttributeChange>,
    logical_id: &RoleLogicalId,
    discord_id: &RoleId,
    attribute: &str,
    current: &str,
    desired: &str,
) {
    if current != desired {
        changes.push(AttributeChange {
            logical_id: logical_id.clone(),
            discord_id: *discord_id,
            attribute: attribute.to_owned(),
            current: current.to_owned(),
            desired: desired.to_owned(),
        });
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
) -> Result<RolePlan, ManagementError> {
    validate_permission_names(definition, &catalog.permission_names)?;
    let actual_roles = catalog
        .roles
        .iter()
        .map(|role| (role.id, role))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    let mut lifecycle = Vec::new();

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
            lifecycle.push(RoleLifecycleChange::Delete {
                logical_id: logical_id.clone(),
                discord_id,
            });
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
            compare_attributes(
                logical_id,
                &discord_id,
                actual,
                &desired_attributes,
                &catalog.default_permissions,
                &catalog.grantable_permissions,
                &mut changes,
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
            lifecycle.push(RoleLifecycleChange::Create {
                logical_id: logical_id.clone(),
                recreated: true,
            });
            continue;
        }

        let Some(discord_id) = state.roles.get(logical_id).copied() else {
            if desired.is_reference() {
                return Err(ManagementError::InvalidState(format!(
                    "参照専用 Role {logical_id} の対応がありません"
                )));
            }
            validate_role_creation(logical_id, desired, &definition.settings_sets.role, catalog)?;
            lifecycle.push(RoleLifecycleChange::Create {
                logical_id: logical_id.clone(),
                recreated: false,
            });
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
        compare_attributes(
            logical_id,
            &discord_id,
            actual,
            &desired_attributes,
            &catalog.default_permissions,
            &catalog.grantable_permissions,
            &mut changes,
        )?;
    }

    for (logical_id, discord_id) in &state.roles {
        if *logical_id == everyone_logical_id() || definition.roles.contains_key(logical_id) {
            continue;
        }
        lifecycle.push(RoleLifecycleChange::Release {
            logical_id: logical_id.clone(),
            discord_id: *discord_id,
        });
    }

    Ok(RolePlan { changes, lifecycle })
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
            .ok_or_else(|| ManagementError::RoleSource(format!("権限 {permission} の Guild 既定値を取得できません")))?;
        let resolved = resolve(value, default);
        if resolved && !catalog.grantable_permissions.contains(permission) {
            return Err(ManagementError::InvalidDefinition(format!(
                "Role {logical_id} に権限 {permission} を付与できません。Bot 自身がこの権限を持っていません"
            )));
        }
    }
    Ok(())
}
