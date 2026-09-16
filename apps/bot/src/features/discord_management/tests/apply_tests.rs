use super::*;

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

impl RoleUpdater for ApplyingFakeRoleSource {
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

#[derive(Clone)]
struct LifecycleFakeRoleSource {
    catalog: Arc<Mutex<RoleCatalog>>,
    catalog_calls: Arc<AtomicUsize>,
    creates: Arc<Mutex<Vec<RoleCreate>>>,
    deletes: Arc<Mutex<Vec<RoleId>>>,
    create_outcome: RoleCreateOutcome,
    create_error_on_call: Option<usize>,
    delete_outcome: RoleDeleteOutcome,
    delete_permission_denied: bool,
    apply_delete: bool,
    create_remove_role: Option<RoleId>,
    catalog_error_on_call: Option<usize>,
    catalog_permission_denied: bool,
}

impl RoleSource for LifecycleFakeRoleSource {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        let call = self.catalog_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.catalog_error_on_call == Some(call) {
            return Err(if self.catalog_permission_denied {
                ManagementError::RoleCatalogPermissionDenied("Role の閲覧権限がありません".to_owned())
            } else {
                ManagementError::RoleSource("取得に失敗しました".to_owned())
            });
        }
        Ok(self.catalog.lock().unwrap().clone())
    }
}

impl RoleUpdater for LifecycleFakeRoleSource {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        role_id: &RoleId,
        update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        let mut catalog = self.catalog.lock().unwrap();
        let role = catalog.roles.iter_mut().find(|role| role.id == *role_id).unwrap();
        update.apply_to(role);
        Ok(RoleUpdateOutcome::Applied)
    }
}

impl RoleLifecycleTarget for LifecycleFakeRoleSource {
    async fn create_role(&self, _guild_id: &GuildId, create: RoleCreate) -> Result<RoleCreateOutcome, ManagementError> {
        let call = {
            let mut creates = self.creates.lock().unwrap();
            creates.push(create.clone());
            creates.len()
        };
        if self.create_error_on_call == Some(call) {
            return Err(ManagementError::RoleSource("作成後の処理に失敗しました".to_owned()));
        }
        if let RoleCreateOutcome::Created(role_id) = self.create_outcome {
            let mut catalog = self.catalog.lock().unwrap();
            catalog.roles.push(RoleSnapshot {
                id: role_id,
                manageable: true,
                name: create.name,
                color: create.color,
                hoist: create.hoist,
                mentionable: create.mentionable,
                permissions: create.permissions,
            });
            if let Some(remove_id) = self.create_remove_role {
                catalog.roles.retain(|role| role.id != remove_id);
            }
        }
        Ok(self.create_outcome)
    }

    async fn delete_role(&self, _guild_id: &GuildId, role_id: &RoleId) -> Result<RoleDeleteOutcome, ManagementError> {
        self.deletes.lock().unwrap().push(*role_id);
        if self.delete_permission_denied {
            return Err(ManagementError::RolePermissionDenied(
                "MANAGE_ROLES がありません".to_owned(),
            ));
        }
        if self.apply_delete {
            self.catalog.lock().unwrap().roles.retain(|role| role.id != *role_id);
        }
        Ok(self.delete_outcome)
    }
}

fn lifecycle_source(catalog: RoleCatalog) -> LifecycleFakeRoleSource {
    LifecycleFakeRoleSource {
        catalog: Arc::new(Mutex::new(catalog)),
        catalog_calls: Arc::new(AtomicUsize::new(0)),
        creates: Arc::new(Mutex::new(Vec::new())),
        deletes: Arc::new(Mutex::new(Vec::new())),
        create_outcome: RoleCreateOutcome::Created(role_id("300")),
        create_error_on_call: None,
        delete_outcome: RoleDeleteOutcome::Deleted,
        delete_permission_denied: false,
        apply_delete: true,
        create_remove_role: None,
        catalog_error_on_call: None,
        catalog_permission_denied: false,
    }
}

/// 作成成功時に新しい Snowflake を state へ記録し、同じ Role を重複作成しない出発点を返すことを保証する。
#[tokio::test]
async fn apply_creates_managed_role_and_returns_new_mapping() {
    let source = lifecycle_source(RoleCatalog {
        roles: vec![role("100", "@everyone")],
        permission_names: known_permissions(["VIEW_CHANNEL"]),
        grantable_permissions: known_permissions(["VIEW_CHANNEL"]),
        default_permissions: known_permission_values([("VIEW_CHANNEL", true)]),
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let plan = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap();

    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", "{}"),
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, RoleApplyStatus::Complete);
    assert_eq!(result.applied, plan);
    assert!(result.pending.is_empty());
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["roles"]["moderator"], "300");
    assert_eq!(source.creates.lock().unwrap().len(), 1);
    assert_eq!(source.creates.lock().unwrap()[0].name, "運営");
    assert_eq!(
        source.creates.lock().unwrap()[0]
            .permissions
            .get(&known_permission("VIEW_CHANNEL")),
        Some(&true),
        "未指定権限は @everyone の既定値を継承します",
    );
}

/// Role の name は引用符と改行を含んでも plan 上で安全に表示する。
#[tokio::test]
async fn role_plan_quotes_name_changes_with_json_escaping() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "旧\n名")],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = "新\"名\n改行"
    "#;
    let plan = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap();

    assert!(plan.render().contains("name: \"旧\\n名\" -> \"新\\\"名\\n改行\""));
}

/// 先行する Role 作成後に後続更新の対象が消えた場合も、作成済み mapping を返して停止する。
#[tokio::test]
async fn mixed_role_apply_returns_confirmed_create_when_later_update_target_is_missing() {
    let mut source = lifecycle_source(RoleCatalog {
        roles: vec![role("100", "@everyone"), role("200", "更新前")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    source.create_remove_role = Some(role_id("200"));
    let definition = r#"
        schema_version = 1
        [roles.a_create]
        name = "新規"
        [roles.b_update]
        name = "更新後"
    "#;
    let state_json = state("100", r#"{"b_update":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();
    assert!(matches!(plan.get(&logical_id("a_create")), Some(Change::Create)));
    assert!(plan.get(&logical_id("b_update")).is_some_and(Change::is_update));

    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(
        matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("b_update") && message.contains("存在しません"))
    );
    assert!(matches!(
        result.applied.get(&logical_id("a_create")),
        Some(Change::Create)
    ));
    assert!(
        result
            .pending
            .get(&logical_id("b_update"))
            .is_some_and(Change::is_update)
    );
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["roles"]["a_create"], "300");
    assert_eq!(returned_state["roles"]["b_update"], "200");
}

/// 先行する Role 作成成功を state に残したまま、後続作成の失敗で停止することを保証する。
#[tokio::test]
async fn create_failure_returns_successful_mappings_only() {
    let mut source = lifecycle_source(RoleCatalog {
        roles: vec![role("100", "@everyone")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    source.create_error_on_call = Some(2);
    let definition = "schema_version = 1\n[roles.first]\nname = \"先行\"\n[roles.second]\nname = \"後続\"\n";
    let state_json = state("100", "{}");
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("作成後の処理")));
    assert_eq!(result.applied.len(), 1);
    assert_eq!(result.pending.len(), 1);
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["roles"]["first"], "300");
    assert!(returned_state["roles"].get("second").is_none());
}

/// 先行する Role 作成成功後に後続作成の ID が既存対応と衝突しても、確定済み mapping を返して停止する。
#[tokio::test]
async fn create_id_collision_returns_successful_mappings_only() {
    let source = lifecycle_source(RoleCatalog {
        roles: vec![role("100", "@everyone")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    let definition = r#"
        schema_version = 1
        [roles.first]
        name = "先行"
        [roles.second]
        name = "後続"
    "#;
    let state_json = state("100", "{}");
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("衝突")));
    assert!(matches!(result.applied.get(&logical_id("first")), Some(Change::Create)));
    assert!(matches!(
        result.pending.get(&logical_id("second")),
        Some(Change::Create)
    ));
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["roles"]["first"], "300");
    assert!(returned_state["roles"].get("second").is_none());
}

/// 作成応答不明時に重複作成せず、入力 state を変更せず返すことを保証する。
#[tokio::test]
async fn unknown_create_response_leaves_mapping_unchanged() {
    let mut source = lifecycle_source(RoleCatalog {
        roles: vec![role("100", "@everyone")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    source.create_outcome = RoleCreateOutcome::ResponseUnknown;
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state_json = state("100", "{}");
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, RoleApplyStatus::CreationResponseUnknown);
    assert_eq!(source.creates.lock().unwrap().len(), 1);
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(
        returned_state,
        serde_json::from_str::<serde_json::Value>(&state_json).unwrap()
    );
    let rerun = plan_roles(&source, guild_id(100), definition, &result.state_json)
        .await
        .unwrap();
    assert!(matches!(rerun.get(&logical_id("moderator")), Some(Change::Create)));
}

/// 定義から Role を外す管理解除が実物を残し、state だけを更新して再適用を無差分にすることを保証する。
#[tokio::test]
async fn omitted_role_is_released_without_deleting_the_discord_role() {
    let source = lifecycle_source(RoleCatalog {
        roles: vec![role("200", "運営")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    let definition = "schema_version = 1\n";
    let state_json = state("100", r#"{"moderator":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    let release = plan
        .get(&logical_id("moderator"))
        .expect("管理対象から外す変更が計画されます");
    assert_eq!(release.discord_id(), Some(role_id("200")));
    assert!(!release.is_update());
    assert!(!release.is_delete());
    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, RoleApplyStatus::Complete);
    assert!(result.pending.is_empty());
    assert_eq!(source.deletes.lock().unwrap().len(), 0);
    assert!(
        serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap()["roles"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(
        plan_roles(&source, guild_id(100), definition, &result.state_json)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(source.catalog.lock().unwrap().roles.len(), 1);
}

/// 明示削除を plan に表示し、削除許可なしでは外部操作を開始しないことを保証する。
#[tokio::test]
async fn deletion_requires_explicit_permission_before_calling_discord() {
    let source = lifecycle_source(RoleCatalog {
        roles: vec![role("200", "不要")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    let definition = "schema_version = 1\n[roles.unused]\nensure = \"absent\"\n";
    let state_json = state("100", r#"{"unused":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    assert!(plan.get(&logical_id("unused")).is_some_and(Change::is_delete));
    assert!(plan.render().contains("削除"));
    assert!(plan.render().contains("影響"));
    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, RoleApplyStatus::DeletionPermissionRequired);
    assert!(source.deletes.lock().unwrap().is_empty());
}

/// Discord API の削除権限不足を、確認不足による削除許可要求とは別の結果として返すことを保証する。
#[tokio::test]
async fn deletion_permission_shortage_is_distinguished_from_confirmation_requirement() {
    let mut source = lifecycle_source(RoleCatalog {
        roles: vec![role("200", "不要")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    source.delete_permission_denied = true;
    let definition = "schema_version = 1\n[roles.unused]\nensure = \"absent\"\n";
    let state_json = state("100", r#"{"unused":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    let result = apply_roles_with_options(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        RoleApplyOptions { allow_deletions: true },
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(matches!(
        result.status,
        RoleApplyStatus::DeletionPermissionDenied(message) if message.contains("MANAGE_ROLES")
    ));
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["roles"]["unused"], "200");
}

/// 明示削除成功で対応を除去し、対応がない同じ論理 ID は通常の作成になることを保証する。
#[tokio::test]
async fn confirmed_deletion_removes_mapping_and_allows_recreation() {
    let source = lifecycle_source(RoleCatalog {
        roles: vec![role("200", "不要")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    let delete_definition = "schema_version = 1\n[roles.unused]\nensure = \"absent\"\n";
    let state_json = state("100", r#"{"unused":"200"}"#);
    let delete_plan = plan_roles(&source, guild_id(100), delete_definition, &state_json)
        .await
        .unwrap();
    let deleted = apply_roles_with_options(
        &source,
        guild_id(100),
        delete_definition,
        &state_json,
        &delete_plan,
        RoleApplyOptions { allow_deletions: true },
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(deleted.status, RoleApplyStatus::Complete);
    let deleted_state: serde_json::Value = serde_json::from_str(&deleted.state_json).unwrap();
    assert!(deleted_state["roles"].get("unused").is_none());
    assert!(
        plan_roles(&source, guild_id(100), delete_definition, &deleted.state_json)
            .await
            .unwrap()
            .is_empty()
    );

    let create_definition = "schema_version = 1\n[roles.unused]\nname = \"再作成\"\n";
    let create_plan = plan_roles(&source, guild_id(100), create_definition, &deleted.state_json)
        .await
        .unwrap();
    assert!(matches!(create_plan.get(&logical_id("unused")), Some(Change::Create)));
    let created = apply_roles(
        &source,
        guild_id(100),
        create_definition,
        &deleted.state_json,
        &create_plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(created.status, RoleApplyStatus::Complete);
    let created_state: serde_json::Value = serde_json::from_str(&created.state_json).unwrap();
    assert_eq!(created_state["roles"]["unused"], "300");
    assert!(created_state["roles"].get("unused").is_some());
}

/// 削除成功直後の state を保持したまま最新構成の取得失敗で停止することを保証する。
#[tokio::test]
async fn delete_refresh_failure_returns_deleted_state_without_rollback() {
    let mut source = lifecycle_source(RoleCatalog {
        roles: vec![role("200", "不要")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    source.catalog_error_on_call = Some(3);
    let definition = "schema_version = 1\n[roles.unused]\nensure = \"absent\"\n";
    let state_json = state("100", r#"{"unused":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    let result = apply_roles_with_options(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        RoleApplyOptions { allow_deletions: true },
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("取得に失敗")));
    assert_eq!(result.applied.len(), 1);
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert!(returned_state["roles"].get("unused").is_none());
    assert!(source.catalog.lock().unwrap().roles.is_empty());
}

/// 削除応答不明時は対応表を維持し、最新の実構成で不在を確認した次回 apply で除去する。
#[tokio::test]
async fn unknown_delete_response_keeps_mapping_until_actual_absence() {
    let mut source = lifecycle_source(RoleCatalog {
        roles: vec![role("200", "不要")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    source.delete_outcome = RoleDeleteOutcome::ResponseUnknown;
    source.apply_delete = false;
    let definition = "schema_version = 1\n[roles.unused]\nensure = \"absent\"\n";
    let state_json = state("100", r#"{"unused":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state_json)
        .await
        .unwrap();

    let unknown = apply_roles_with_options(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        RoleApplyOptions { allow_deletions: true },
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(unknown.status, RoleApplyStatus::DeletionResponseUnknown);
    let unknown_state: serde_json::Value = serde_json::from_str(&unknown.state_json).unwrap();
    assert_eq!(unknown_state["roles"]["unused"], "200");

    source.catalog.lock().unwrap().roles.clear();
    let rerun = plan_roles(&source, guild_id(100), definition, &unknown.state_json)
        .await
        .unwrap();
    assert!(matches!(
        rerun.get(&logical_id("unused")),
        Some(Change::Delete { discord_id }) if *discord_id == role_id("200")
    ));

    let resolved = apply_roles_with_options(
        &source,
        guild_id(100),
        definition,
        &unknown.state_json,
        &rerun,
        RoleApplyOptions { allow_deletions: true },
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(resolved.status, RoleApplyStatus::Complete);
    let resolved_state: serde_json::Value = serde_json::from_str(&resolved.state_json).unwrap();
    assert!(resolved_state["roles"].get("unused").is_none());
    assert_eq!(source.deletes.lock().unwrap().len(), 1);
}

/// active 対応先が予期せず消えた場合、自動再作成せず state エラーで停止することを保証する。
#[tokio::test]
async fn active_role_disappearance_is_not_treated_as_creation() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let error = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("予期せず消失")));
}

/// 新規 Role に必須の name がない場合、外部操作前に定義エラーにすることを保証する。
#[tokio::test]
async fn new_managed_role_requires_a_name() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };
    let definition = "schema_version = 1\n[roles.moderator]\nhoist = true\n";
    let error = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("name") && message.contains("必要"))
    );
}

/// Botが持たない権限のfalseからtrueへの変更をplanで拒否し、Discord APIの失敗を事前に示す。
#[tokio::test]
async fn plan_rejects_granting_a_permission_the_bot_does_not_have() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: known_permissions(["SEND_MESSAGES", "VIEW_CHANNEL"]),
            grantable_permissions: known_permissions(["VIEW_CHANNEL"]),
            default_permissions: known_permission_values([("SEND_MESSAGES", false), ("VIEW_CHANNEL", true)]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let definition = "schema_version = 1\n[roles.moderator.permissions]\nSEND_MESSAGES = true\n";

    let error = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("SEND_MESSAGES") && message.contains("Bot 自身"))
    );
}

/// Botが現在持たない権限でも、対象Roleから削除する変更は付与ではないためplanできることを保証する。
#[tokio::test]
async fn plan_allows_removing_a_permission_the_bot_does_not_have() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: known_permissions(["SEND_MESSAGES", "VIEW_CHANNEL"]),
            grantable_permissions: BTreeSet::new(),
            default_permissions: known_permission_values([("SEND_MESSAGES", false), ("VIEW_CHANNEL", true)]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let definition = "schema_version = 1\n[roles.moderator.permissions]\nVIEW_CHANNEL = false\n";

    let plan = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap();

    assert_eq!(plan.len(), 1);
    let attributes = plan
        .get(&logical_id("moderator"))
        .and_then(Change::attributes)
        .expect("権限削除は対象 Role の一つの Update にまとまります");
    assert!(attributes.permissions().contains_key(&known_permission("VIEW_CHANNEL")));
}

/// stateに対応を持たないeveryoneもGuild IDへ解決され、planどおりにapplyされることを保証する。
#[tokio::test]
async fn apply_resolves_everyone_without_a_state_mapping() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("100", "@everyone")],
            permission_names: known_permissions(["SEND_MESSAGES", "VIEW_CHANNEL"]),
            grantable_permissions: known_permissions(["SEND_MESSAGES", "VIEW_CHANNEL"]),
            default_permissions: known_permission_values([("SEND_MESSAGES", false), ("VIEW_CHANNEL", true)]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let definition = "schema_version = 1\n[roles.everyone.permissions]\nSEND_MESSAGES = true\n";
    let state = state("100", "{}");
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();

    let result = apply_role_updates(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, RoleApplyStatus::Complete);
    assert_eq!(result.applied, plan);
    assert!(result.pending.is_empty());
    assert_eq!(source.updates.lock().unwrap().len(), 1);
}

/// applyが明示属性だけを更新し、省略された権限や属性を現在値のまま保持することを保証する。
#[tokio::test]
async fn apply_updates_only_explicit_attributes_and_preserves_omitted_permissions() {
    let mut moderator = role("200", "運営");
    moderator.permissions = known_permission_values([("VIEW_CHANNEL", false), ("MANAGE_MESSAGES", true)]);
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![moderator],
            permission_names: known_permissions(["VIEW_CHANNEL", "MANAGE_MESSAGES"]),
            grantable_permissions: known_permissions(["VIEW_CHANNEL", "MANAGE_MESSAGES"]),
            default_permissions: known_permission_values([("VIEW_CHANNEL", false), ("MANAGE_MESSAGES", false)]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = "モデレーター"
        [roles.moderator.permissions]
        VIEW_CHANNEL = true
    "#;
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();

    let result = apply_role_updates(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        std::time::Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, RoleApplyStatus::Complete);
    assert_eq!(result.applied, plan);
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
        Some(known_permission_values([
            ("MANAGE_MESSAGES", true),
            ("VIEW_CHANNEL", true),
        ]))
    );
}

/// apply_rolesを公開入口として維持し、属性更新の共通処理へ委譲できることを保証する。
#[tokio::test]
async fn apply_roles_remains_the_entrypoint_for_attribute_updates() {
    let source = lifecycle_source(RoleCatalog {
        roles: vec![role("200", "運営")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();

    let result = apply_roles(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, RoleApplyStatus::Complete);
    assert_eq!(result.applied, plan);
    assert_eq!(source.catalog.lock().unwrap().roles[0].name, "モデレーター");
}

/// 確認後に管理対象の現在値が変わった場合、古いplanを適用せず再planを要求することを保証する。
#[tokio::test]
async fn apply_requires_a_new_plan_when_managed_attributes_changed_after_confirmation() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let confirmed_plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();
    source.catalog.lock().unwrap().roles[0].name = "外部変更".to_owned();

    let result = apply_role_updates(
        &source,
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

/// 更新応答が不明な場合、state の論理 ID 対応を変更せず応答不明を返す。
#[tokio::test]
async fn unknown_update_response_preserves_state_mapping() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::ResponseUnknown,
        apply_update: false,
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();

    let result = apply_role_updates(
        &source,
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
    assert_eq!(result.pending, plan);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap(),
        serde_json::from_str::<serde_json::Value>(&state).unwrap()
    );

    let changed_definition = "schema_version = 1\n[roles.moderator]\nname = \"別の名前\"\n";
    let changed_plan = plan_roles(&source, guild_id(100), changed_definition, &result.state_json)
        .await
        .unwrap();
    assert!(changed_plan.get(&logical_id("moderator")).is_some());
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

impl RoleUpdater for NeverCompletesRoleUpdate {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        _role_id: &RoleId,
        _update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        std::future::pending().await
    }
}

/// 更新期限超過時に応答不明として返し、入力 state の対応を維持する。
#[tokio::test]
async fn update_deadline_returns_unknown_progress_without_state_change() {
    let catalog = RoleCatalog {
        roles: vec![role("200", "運営")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
        default_permissions: BTreeMap::new(),
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let timing_out_source = NeverCompletesRoleUpdate {
        catalog: catalog.clone(),
    };
    let plan = plan_roles(&timing_out_source, guild_id(100), definition, &state)
        .await
        .unwrap();

    let timed_out = apply_role_updates(
        &timing_out_source,
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
    assert_eq!(timed_out.pending, plan);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&timed_out.state_json).unwrap(),
        serde_json::from_str::<serde_json::Value>(&state).unwrap()
    );
}

/// 処理開始時点で期限切れなら更新を一件も始めず、取得済みの最新stateを返すことを保証する。
#[tokio::test]
async fn expired_processing_budget_starts_no_updates_and_returns_latest_state() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();

    let result = apply_role_updates(&source, guild_id(100), definition, &state, &plan, Instant::now())
        .await
        .unwrap();

    assert_eq!(result.status, RoleApplyStatus::DeadlineExceeded);
    assert!(result.applied.is_empty());
    assert_eq!(result.pending, plan);
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

impl RoleUpdater for FailsOnSecondUpdate {
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

/// 途中の更新失敗で処理を停止し、適用済みと未適用の差分を正確に分けて返すことを保証する。
#[tokio::test]
async fn apply_stops_at_first_failure_and_reports_successful_and_remaining_changes() {
    let source = FailsOnSecondUpdate {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "A"), role("201", "B")],
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let definition = "schema_version = 1\n[roles.a]\nname = \"new A\"\n[roles.b]\nname = \"new B\"\n";
    let state = state("100", r#"{"a":"200","b":"201"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();

    let result = apply_role_updates(
        &source,
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
            .map(|(logical_id, _)| logical_id.to_string())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert_eq!(
        result
            .pending
            .iter()
            .map(|(logical_id, _)| logical_id.to_string())
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

impl RoleUpdater for RefetchFailsAfterAppliedUpdate {
    async fn update_role(
        &self,
        _guild_id: &GuildId,
        _role_id: &RoleId,
        _update: RoleUpdate,
    ) -> Result<RoleUpdateOutcome, ManagementError> {
        Ok(RoleUpdateOutcome::Applied)
    }
}

/// Discordが更新受理を返した後の再取得失敗でも、確定済み更新を未適用へ戻さないことを保証する。
#[tokio::test]
async fn acknowledged_update_is_reported_as_success_even_when_refetch_fails() {
    let source = RefetchFailsAfterAppliedUpdate {
        catalog: RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        },
        catalog_calls: Arc::new(AtomicUsize::new(0)),
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();

    let result = apply_role_updates(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(matches!(result.status, RoleApplyStatus::Failed(message) if message.contains("refetch failed")));
    assert_eq!(result.applied, plan);
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

impl RoleUpdater for BlockingRoleTarget {
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

/// 同一Guildへの並行applyを待機させず拒否し、競合更新と二重適用を防ぐ。
#[tokio::test]
async fn concurrent_apply_for_the_same_guild_is_rejected_without_waiting() {
    let apply_lock = GuildApplyLock::default();
    let (started_tx, mut started_rx) = mpsc::unbounded_channel();
    let source = BlockingRoleTarget {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })),
        started: started_tx,
        release: Arc::new(Semaphore::new(0)),
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"モデレーター\"\n";
    let state = state("100", r#"{"moderator":"200"}"#);
    let plan = plan_roles(&source, guild_id(100), definition, &state).await.unwrap();
    let first_source = source.clone();
    let first_plan = plan.clone();
    let first_state = state.clone();
    let first_apply_lock = apply_lock.clone();
    let first = tokio::spawn(async move {
        let vocabulary = test_permission_vocabulary();
        RoleApplyWorkflow::new(&first_apply_lock, &first_source, &vocabulary)
            .apply_role_updates(
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
        RoleApplyWorkflow::new(&apply_lock, &source, &test_permission_vocabulary()).apply_role_updates(
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
