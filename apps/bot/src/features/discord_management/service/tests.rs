use super::*;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, mpsc};

struct StatefulFakeRoleSource {
    guild_id: String,
    roles: Vec<RoleSnapshot>,
}

impl RoleSource for StatefulFakeRoleSource {
    async fn role_catalog(&self, guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        if guild_id.to_string() != self.guild_id {
            return Err(ManagementError::RoleSource(format!("Guild {guild_id} は存在しません")));
        }
        let permission_names = self
            .roles
            .iter()
            .flat_map(|role| role.permissions.keys().cloned())
            .collect::<BTreeSet<_>>();
        let default_permissions = self
            .roles
            .iter()
            .find(|role| role.id.get() == guild_id.get())
            .map(|role| role.permissions.clone())
            .unwrap_or_else(|| permission_names.iter().map(|name| (name.clone(), false)).collect());
        Ok(RoleCatalog {
            roles: self.roles.clone(),
            permission_names,
            default_permissions,
        })
    }
}

fn role(id: &str, name: &str) -> RoleSnapshot {
    RoleSnapshot {
        id: id.parse().unwrap(),
        manageable: true,
        name: name.to_owned(),
        color: 0,
        hoist: false,
        mentionable: false,
        permissions: BTreeMap::from([("SEND_MESSAGES".to_owned(), false), ("VIEW_CHANNEL".to_owned(), true)]),
    }
}

fn literal_string(value: &Option<ManagedValue<String>>) -> Option<&str> {
    match value {
        Some(ManagedValue::Value(value)) => Some(value),
        _ => None,
    }
}

fn literal_bool(value: Option<&ManagedValue<bool>>) -> Option<bool> {
    match value {
        Some(ManagedValue::Value(value)) => Some(*value),
        _ => None,
    }
}

fn state(guild_id: &str, roles: &str) -> String {
    format!(r#"{{"schema_version":1,"guild_id":"{guild_id}","roles":{roles}}}"#)
}

fn logical_id(value: &str) -> RoleLogicalId {
    RoleLogicalId::parse(value).unwrap()
}

fn role_id(value: &str) -> RoleId {
    value.parse().unwrap()
}

fn guild_id(value: u64) -> GuildId {
    GuildId::new(value)
}

#[tokio::test]
async fn initial_export_uses_snowflakes_for_duplicate_role_names() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営"), role("201", "運営")],
    });

    let files = service.export_roles(guild_id(100), None).await.unwrap();
    let definition: DefinitionFile = toml::from_str(&files.definition_toml).unwrap();
    let state: StateFile = serde_json::from_str(&files.state_json).unwrap();

    assert_eq!(definition.schema_version, SCHEMA_VERSION);
    assert_eq!(
        literal_string(&definition.roles[&logical_id("role_200")].attributes.name),
        Some("運営")
    );
    assert_eq!(
        literal_string(&definition.roles[&logical_id("role_201")].attributes.name),
        Some("運営")
    );
    assert!(!definition.roles[&logical_id("role_200")]
        .attributes
        .permissions
        .contains_key("SEND_MESSAGES"));
    assert_eq!(state.guild_id.get(), 100);
    assert_eq!(state.roles[&logical_id("role_200")].get(), 200);
    assert_eq!(state.roles[&logical_id("role_201")].get(), 201);
}

#[tokio::test]
async fn everyone_export_contains_only_permissions_and_keeps_false_values() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    });

    let files = service.export_roles(guild_id(100), None).await.unwrap();
    let definition: DefinitionFile = toml::from_str(&files.definition_toml).unwrap();
    let state: StateFile = serde_json::from_str(&files.state_json).unwrap();
    let everyone = &definition.roles[&logical_id("everyone")].attributes;

    assert!(everyone.name.is_none());
    assert!(everyone.color.is_none());
    assert!(everyone.hoist.is_none());
    assert!(everyone.mentionable.is_none());
    assert_eq!(literal_bool(everyone.permissions.get("VIEW_CHANNEL")), Some(true));
    assert_eq!(literal_bool(everyone.permissions.get("SEND_MESSAGES")), Some(false));
    assert!(!state.roles.contains_key(&logical_id("everyone")));

    let plan = service
        .plan_roles(guild_id(100), &files.definition_toml, &files.state_json)
        .await
        .unwrap();
    assert!(plan.changes.is_empty());
}

#[tokio::test]
async fn everyone_rejects_non_permission_managed_attributes() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    });
    let definition = r#"
        schema_version = 1
        [roles.everyone]
        name = "renamed"
    "#;

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("@everyone") && message.contains("権限")));
}

#[tokio::test]
async fn re_export_preserves_logical_ids_from_input_state() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "名称変更後")],
    });
    let previous_state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": { "moderator": "200" }
    }"#;

    let files = service.export_roles(guild_id(100), Some(previous_state)).await.unwrap();
    let definition: DefinitionFile = toml::from_str(&files.definition_toml).unwrap();
    let state: StateFile = serde_json::from_str(&files.state_json).unwrap();

    assert_eq!(
        literal_string(&definition.roles[&logical_id("moderator")].attributes.name),
        Some("名称変更後")
    );
    assert_eq!(state.roles[&logical_id("moderator")].get(), 200);
    assert!(!definition.roles.contains_key(&logical_id("role_200")));
}

#[tokio::test]
async fn exported_definition_has_no_plan_changes() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let service = RoleManagementService::new(source);
    let files = service.export_roles(guild_id(100), None).await.unwrap();

    let plan = service
        .plan_roles(guild_id(100), &files.definition_toml, &files.state_json)
        .await
        .unwrap();

    assert!(plan.changes.is_empty());
}

#[tokio::test]
async fn reference_role_may_target_an_unmanageable_guild_role() {
    let mut external_role = role("200", "外部 Bot");
    external_role.manageable = false;
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![external_role],
    });
    let definition = r#"
        schema_version = 1
        [roles.external_bot]
        mode = "reference"
    "#;

    let plan = service
        .plan_roles(guild_id(100), definition, &state("100", r#"{"external_bot":"200"}"#))
        .await
        .unwrap();

    assert!(plan.changes.is_empty());
}

#[tokio::test]
async fn everyone_role_may_be_used_as_a_reference() {
    let mut everyone = role("100", "@everyone");
    everyone.manageable = false;
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![everyone],
    });
    let definition = r#"
        schema_version = 1
        [roles.everyone]
        mode = "reference"
    "#;

    let plan = service
        .plan_roles(guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap();

    assert!(plan.changes.is_empty());
}

#[tokio::test]
async fn permission_default_uses_everyone_role_value() {
    let mut everyone = role("100", "@everyone");
    everyone.manageable = false;
    let mut moderator = role("200", "運営");
    moderator.permissions.insert("VIEW_CHANNEL".to_owned(), false);
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![everyone, moderator],
    });
    let definition = r#"
        schema_version = 1
        [roles.moderator.permissions]
        VIEW_CHANNEL = { default = true }
    "#;

    let plan = service
        .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
        .await
        .unwrap();

    assert_eq!(plan.changes[0].attribute, "permissions.VIEW_CHANNEL");
    assert_eq!(plan.changes[0].current, "false");
    assert_eq!(plan.changes[0].desired, "true");
}

#[tokio::test]
async fn export_omits_unmanageable_roles_but_keeps_manageable_roles() {
    let mut external_role = role("200", "外部 Bot");
    external_role.manageable = false;
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![external_role, role("201", "運営")],
    });

    let files = service.export_roles(guild_id(100), None).await.unwrap();
    let definition: DefinitionFile = toml::from_str(&files.definition_toml).unwrap();

    assert!(!definition.roles.contains_key(&logical_id("role_200")));
    assert!(definition.roles.contains_key(&logical_id("role_201")));
}

#[tokio::test]
async fn role_settings_set_attributes_are_planned() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let definition = r#"
        schema_version = 1

        [settings_sets.role.staff]
        hoist = true

        [roles.moderator]
        settings_sets = ["staff"]
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": { "moderator": "200" }
    }"#;

    let plan = service.plan_roles(guild_id(100), definition, state).await.unwrap();

    assert_eq!(
        plan.changes,
        vec![AttributeChange {
            logical_id: logical_id("moderator"),
            discord_id: role_id("200"),
            attribute: "hoist".to_owned(),
            current: "false".to_owned(),
            desired: "true".to_owned(),
        }]
    );
}

#[tokio::test]
async fn direct_attributes_override_later_settings_sets_and_omitted_attributes_are_retained() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let definition = r#"
        schema_version = 1

        [settings_sets.role.first]
        hoist = true
        mentionable = true

        [settings_sets.role.second]
        mentionable = false

        [roles.moderator]
        settings_sets = ["first", "second"]
        mentionable = true
    "#;

    let plan = service
        .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
        .await
        .unwrap();

    assert_eq!(plan.changes.len(), 2);
    assert!(plan.changes.iter().any(|change| change.attribute == "hoist"));
    assert!(plan.changes.iter().any(|change| change.attribute == "mentionable"));
    assert!(!plan.changes.iter().any(|change| change.attribute == "name"));
}

#[tokio::test]
async fn default_specifiers_are_resolved_to_schema_version_values() {
    let mut actual = role("200", "運営");
    actual.color = 0x12_34_56;
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![actual],
    });
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = { default = true }
        color = { default = true }
    "#;

    let plan = service
        .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
        .await
        .unwrap();

    assert!(plan.changes.iter().any(|change| {
        change.attribute == "name" && change.current == "運営" && change.desired == "new role"
    }));
    assert!(
        plan.changes
            .iter()
            .any(|change| { change.attribute == "color" && change.current == "1193046" && change.desired == "0" })
    );
}

#[tokio::test]
async fn guild_mismatch_is_reported_before_reading_discord() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let error = service
        .plan_roles(guild_id(100), "schema_version = 1", &state("999", "{}"))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ManagementError::GuildMismatch {
            state_guild_id: guild_id(999),
            actual_guild_id: guild_id(100),
        }
    );
}

#[tokio::test]
async fn non_numeric_state_guild_id_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let error = service
        .plan_roles(guild_id(100), "schema_version = 1", &state("not-a-snowflake", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("Guild ID")));
}

#[tokio::test]
async fn everyone_mapping_in_state_is_rejected() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    });
    let error = service
        .plan_roles(
            guild_id(100),
            "schema_version = 1\n[roles.everyone]",
            &state("100", r#"{"everyone":"100"}"#),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("everyone") && message.contains("state に含めず")));
}

#[tokio::test]
async fn duplicate_snowflakes_in_state_are_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let error = service
        .export_roles(
            guild_id(100),
            Some(&state("100", r#"{"moderator":"200","staff":"200"}"#)),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("同じ Snowflake 200")));
}

#[tokio::test]
async fn duplicate_logical_id_keys_in_state_are_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": { "moderator": "200", "moderator": "201" }
    }"#;

    let error = service.export_roles(guild_id(100), Some(state)).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("論理 ID moderator が重複"))
    );
}

#[tokio::test]
async fn generated_logical_id_collision_with_stale_state_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let error = service
        .export_roles(guild_id(100), Some(&state("100", r#"{"role_200":"999"}"#)))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("論理 ID role_200")));
}

#[tokio::test]
async fn unknown_definition_keys_are_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        unknown = true
    "#;

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(_)));
}

#[tokio::test]
async fn invalid_unreferenced_settings_set_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let definition = r#"
        schema_version = 1
        [settings_sets.role.unused]
        color = 16777216
    "#;

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("color")));
}

#[tokio::test]
async fn false_default_in_unreferenced_settings_set_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let definition = r#"
        schema_version = 1
        [settings_sets.role.unused]
        hoist = { default = false }
    "#;

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("default")));
}

#[tokio::test]
async fn unknown_permission_in_unreferenced_settings_set_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    });
    let definition = r#"
        schema_version = 1
        [settings_sets.role.unused.permissions]
        NOT_A_DISCORD_PERMISSION = true
    "#;

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未知の権限")));
}

#[tokio::test]
async fn unsupported_definition_version_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });

    let error = service
        .plan_roles(guild_id(100), "schema_version = 2", &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("schema_version")));
}

#[test]
fn editor_sample_matches_the_supported_definition_shape() {
    let sample = include_str!("../../../../../../docs/examples/discord-role-management.toml");
    toml::from_str::<DefinitionFile>(sample).unwrap();
}

#[tokio::test]
async fn unknown_settings_set_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        settings_sets = ["missing"]
    "#;

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未知の設定セット")));
}

#[tokio::test]
async fn reference_role_with_managed_attributes_is_reported() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = r#"
        schema_version = 1
        [roles.external]
        mode = "reference"
        hoist = true
    "#;

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("参照専用 Role")));
}

#[derive(Clone)]
struct ApplyingFakeRoleSource {
    catalog: Arc<Mutex<RoleCatalog>>,
    updates: Arc<Mutex<Vec<RoleUpdate>>>,
    outcome: RoleUpdateOutcome,
    apply_update: bool,
}

impl RoleSource for ApplyingFakeRoleSource {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        Ok(self.catalog.lock().unwrap().clone())
    }
}

impl RoleTarget for ApplyingFakeRoleSource {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        self.updates.lock().unwrap().push(update.clone());
        if self.apply_update {
            let mut catalog = self.catalog.lock().unwrap();
            let role = catalog.roles.iter_mut().find(|role| role.id == *role_id).unwrap();
            update.apply_to(role);
        }
        Ok(self.outcome)
    }
}

#[tokio::test]
async fn apply_updates_only_explicit_attributes_and_preserves_omitted_permissions() {
    let mut moderator = role("200", "運営");
    moderator.permissions =
        BTreeMap::from([("VIEW_CHANNEL".to_owned(), false), ("MANAGE_MESSAGES".to_owned(), true)]);
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![moderator],
            permission_names: BTreeSet::from(["VIEW_CHANNEL".to_owned(), "MANAGE_MESSAGES".to_owned()]),
            default_permissions: BTreeMap::from([
                ("VIEW_CHANNEL".to_owned(), false),
                ("MANAGE_MESSAGES".to_owned(), false),
            ]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let service = RoleManagementService::new(source.clone());
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = "モデレーター"
        [roles.moderator.permissions]
        VIEW_CHANNEL = true
    "#;
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

    let result = service
        .apply_roles(
            guild_id(100),
            definition,
            &state,
            &plan,
            std::time::Instant::now() + Duration::from_secs(60),
        )
        .await
        .unwrap();

    assert_eq!(result.status, RoleApplyStatus::Complete);
    assert_eq!(result.applied, plan.changes);
    assert!(result.pending.is_empty());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap(),
        serde_json::json!({
            "schema_version": 1,
            "guild_id": "100",
            "roles": { "moderator": "200" }
        })
    );
    let updates = source.updates.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].name.as_deref(), Some("モデレーター"));
    assert_eq!(
        updates[0].permissions,
        Some(BTreeMap::from([
            ("MANAGE_MESSAGES".to_owned(), true),
            ("VIEW_CHANNEL".to_owned(), true),
        ]))
    );
}

#[tokio::test]
async fn apply_requires_a_new_plan_when_managed_attributes_changed_after_confirmation() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let service = RoleManagementService::new(source.clone());
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let confirmed_plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();
    source.catalog.lock().unwrap().roles[0].name = "外部変更".to_owned();

    let result = service
        .apply_roles(
            guild_id(100),
            definition,
            &state,
            &confirmed_plan,
            Instant::now() + Duration::from_secs(60),
        )
        .await
        .unwrap();

    assert_eq!(result.status, RoleApplyStatus::ReplanRequired);
    assert!(result.applied.is_empty());
    assert!(source.updates.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_update_response_stops_when_refetched_value_does_not_match() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::ResponseUnknown,
        apply_update: false,
    };
    let service = RoleManagementService::new(source);
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

    let result = service
        .apply_roles(
            guild_id(100),
            definition,
            &state,
            &plan,
            Instant::now() + Duration::from_secs(60),
        )
        .await
        .unwrap();

    assert_eq!(result.status, RoleApplyStatus::ResponseUnknown);
    assert!(result.applied.is_empty());
    assert_eq!(result.pending, plan.changes);
}

#[derive(Clone)]
struct NeverCompletesRoleUpdate {
    catalog: RoleCatalog,
}

impl RoleSource for NeverCompletesRoleUpdate {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        Ok(self.catalog.clone())
    }
}

impl RoleTarget for NeverCompletesRoleUpdate {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        _role_id: &RoleId,
        _update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn update_deadline_returns_unknown_progress_that_can_be_resubmitted() {
    let catalog = RoleCatalog {
        roles: vec![role("200", "運営")],
        permission_names: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let timing_out_service = RoleManagementService::new(NeverCompletesRoleUpdate {
        catalog: catalog.clone(),
    });
    let plan = timing_out_service
        .plan_roles(guild_id(100), definition, &state)
        .await
        .unwrap();

    let timed_out = timing_out_service
        .apply_roles(
            guild_id(100),
            definition,
            &state,
            &plan,
            Instant::now() + Duration::from_millis(10),
        )
        .await
        .unwrap();
    assert_eq!(timed_out.status, RoleApplyStatus::ResponseUnknown);
    assert!(timed_out.applied.is_empty());
    assert_eq!(timed_out.pending, plan.changes);

    let resubmitted_source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(catalog)),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let resubmitted = RoleManagementService::new(resubmitted_source)
        .apply_roles(
            guild_id(100),
            definition,
            &timed_out.state_json,
            &plan,
            Instant::now() + Duration::from_secs(60),
        )
        .await
        .unwrap();

    assert_eq!(resubmitted.status, RoleApplyStatus::Complete);
    assert_eq!(resubmitted.applied, plan.changes);
    assert!(resubmitted.pending.is_empty());
}

#[tokio::test]
async fn expired_processing_budget_starts_no_updates_and_returns_latest_state() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let service = RoleManagementService::new(source.clone());
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

    let result = service
        .apply_roles(guild_id(100), definition, &state, &plan, Instant::now())
        .await
        .unwrap();

    assert_eq!(result.status, RoleApplyStatus::DeadlineExceeded);
    assert!(result.applied.is_empty());
    assert_eq!(result.pending, plan.changes);
    assert!(source.updates.lock().unwrap().is_empty());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap()["guild_id"],
        "100"
    );
}

#[derive(Clone)]
struct FailsOnSecondUpdate {
    catalog: Arc<Mutex<RoleCatalog>>,
    calls: Arc<AtomicUsize>,
}

impl RoleSource for FailsOnSecondUpdate {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        Ok(self.catalog.lock().unwrap().clone())
    }
}

impl RoleTarget for FailsOnSecondUpdate {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
            return Err(ManagementError::RoleSource("injected failure".to_owned()));
        }
        let mut catalog = self.catalog.lock().unwrap();
        update.apply_to(catalog.roles.iter_mut().find(|role| role.id == *role_id).unwrap());
        Ok(RoleUpdateOutcome::Applied)
    }
}

#[tokio::test]
async fn apply_stops_at_first_failure_and_reports_successful_and_pending_changes() {
    let source = FailsOnSecondUpdate {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "A"), role("201", "B")],
            permission_names: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let service = RoleManagementService::new(source);
    let definition = "schema_version = 1\n[roles.a]\nname = \"new A\"\n[roles.b]\nname = \"new B\"\n";
    let state = state("100", r#"{"a":"200","b":"201"}"#);
    let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

    let result = service
        .apply_roles(
            guild_id(100),
            definition,
            &state,
            &plan,
            Instant::now() + Duration::from_secs(60),
        )
        .await
        .unwrap();

    assert!(matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("injected failure")));
    assert_eq!(
        result
            .applied
            .iter()
            .map(|change| change.logical_id.to_string())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert_eq!(
        result
            .pending
            .iter()
            .map(|change| change.logical_id.to_string())
            .collect::<Vec<_>>(),
        ["b"]
    );
}

#[derive(Clone)]
struct RefetchFailsAfterAppliedUpdate {
    catalog: RoleCatalog,
    catalog_calls: Arc<AtomicUsize>,
}

impl RoleSource for RefetchFailsAfterAppliedUpdate {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        if self.catalog_calls.fetch_add(1, Ordering::SeqCst) == 2 {
            Err(ManagementError::RoleSource("refetch failed".to_owned()))
        } else {
            Ok(self.catalog.clone())
        }
    }
}

impl RoleTarget for RefetchFailsAfterAppliedUpdate {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        _role_id: &RoleId,
        _update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        Ok(RoleUpdateOutcome::Applied)
    }
}

#[tokio::test]
async fn acknowledged_update_is_reported_as_success_even_when_refetch_fails() {
    let source = RefetchFailsAfterAppliedUpdate {
        catalog: RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        },
        catalog_calls: Arc::new(AtomicUsize::new(0)),
    };
    let service = RoleManagementService::new(source);
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = service.plan_roles(guild_id(100), definition, &state).await.unwrap();

    let result = service
        .apply_roles(
            guild_id(100),
            definition,
            &state,
            &plan,
            Instant::now() + Duration::from_secs(60),
        )
        .await
        .unwrap();

    assert!(matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("refetch failed")));
    assert_eq!(result.applied, plan.changes);
    assert!(result.pending.is_empty());
}

#[derive(Clone)]
struct BlockingRoleTarget {
    catalog: Arc<Mutex<RoleCatalog>>,
    started: mpsc::UnboundedSender<()>,
    release: Arc<Semaphore>,
}

impl RoleSource for BlockingRoleTarget {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        Ok(self.catalog.lock().unwrap().clone())
    }
}

impl RoleTarget for BlockingRoleTarget {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        self.started.send(()).unwrap();
        self.release.acquire().await.unwrap().forget();
        let mut catalog = self.catalog.lock().unwrap();
        update.apply_to(catalog.roles.iter_mut().find(|role| role.id == *role_id).unwrap());
        Ok(RoleUpdateOutcome::Applied)
    }
}

#[tokio::test]
async fn concurrent_apply_for_the_same_guild_is_rejected_without_waiting() {
    let (started_tx, mut started_rx) = mpsc::unbounded_channel();
    let source = BlockingRoleTarget {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        started: started_tx,
        release: Arc::new(Semaphore::new(0)),
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = RoleManagementService::new(source.clone())
        .plan_roles(guild_id(100), definition, &state)
        .await
        .unwrap();
    let first_source = source.clone();
    let first_plan = plan.clone();
    let first_state = state.clone();
    let first = tokio::spawn(async move {
        RoleManagementService::new(first_source)
            .apply_roles(
                guild_id(100),
                definition,
                &first_state,
                &first_plan,
                Instant::now() + Duration::from_secs(60),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
        .await
        .expect("first apply did not reach the update")
        .unwrap();

    let second = tokio::time::timeout(
        Duration::from_secs(1),
        RoleManagementService::new(source.clone()).apply_roles(
            guild_id(100),
            definition,
            &state,
            &plan,
            Instant::now() + Duration::from_secs(60),
        ),
    )
    .await
    .expect("second apply waited instead of being rejected")
    .unwrap();
    assert_eq!(second.status, RoleApplyStatus::GuildBusy);

    source.release.add_permits(1);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .expect("first apply did not finish after release")
            .unwrap()
            .unwrap()
            .status,
        RoleApplyStatus::Complete
    );
}
