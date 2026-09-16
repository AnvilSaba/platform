use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::*;

#[derive(Clone)]
struct ChannelCatalogSource {
    catalog: ChannelCatalog,
}

impl ChannelSource for ChannelCatalogSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.clone())
    }
}

#[derive(Clone)]
struct PermissionAwareChannelSource {
    catalog: ChannelCatalog,
    can_manage_roles: bool,
}

impl ChannelSource for PermissionAwareChannelSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.clone())
    }

    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(self.can_manage_roles)
    }
}

#[derive(Clone)]
struct BlockingCanManageRolesChannelSource {
    catalog: ChannelCatalog,
}

impl ChannelSource for BlockingCanManageRolesChannelSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.clone())
    }

    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(true)
    }
}

impl ChannelUpdater for BlockingCanManageRolesChannelSource {
    async fn update_channel(
        &self,
        _guild_id: &GuildId,
        _channel_id: &ChannelId,
        _update: ChannelUpdate,
    ) -> Result<ChannelUpdateOutcome, ManagementError> {
        unreachable!("can_manage_roles の期限超過後に Channel 更新へ進みません")
    }
}

impl ChannelLifecycleTarget for BlockingCanManageRolesChannelSource {
    async fn create_channel(
        &self,
        _guild_id: &GuildId,
        _create: ChannelCreate,
    ) -> Result<ChannelCreateOutcome, ManagementError> {
        unreachable!("can_manage_roles の期限超過後に Channel 作成へ進みません")
    }

    async fn delete_channel(
        &self,
        _guild_id: &GuildId,
        _channel_id: &ChannelId,
    ) -> Result<ChannelDeleteOutcome, ManagementError> {
        unreachable!("can_manage_roles の期限超過後に Channel 削除へ進みません")
    }
}

#[derive(Clone)]
struct ApplyingFakeChannelSource {
    catalog: Arc<Mutex<ChannelCatalog>>,
    updates: Arc<Mutex<Vec<ChannelUpdate>>>,
    creates: Arc<Mutex<Vec<ChannelCreate>>>,
    deletes: Arc<Mutex<Vec<ChannelId>>>,
    next_id: Arc<Mutex<u64>>,
    create_outcome: ChannelCreateOutcome,
    create_remove_channel: Option<ChannelId>,
    delete_outcome: ChannelDeleteOutcome,
    apply_delete: bool,
}

impl ChannelSource for ApplyingFakeChannelSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.lock().unwrap().clone())
    }
}

impl ChannelUpdater for ApplyingFakeChannelSource {
    async fn update_channel(
        &self,
        _guild_id: &GuildId,
        channel_id: &ChannelId,
        update: ChannelUpdate,
    ) -> Result<ChannelUpdateOutcome, ManagementError> {
        self.updates.lock().unwrap().push(update.clone());
        let mut catalog = self.catalog.lock().unwrap();
        let channel = catalog
            .channels
            .iter_mut()
            .find(|channel| channel.id == *channel_id)
            .expect("更新対象 Channel がカタログに存在します");
        update.apply_to(channel);
        Ok(ChannelUpdateOutcome::Applied)
    }
}

impl ChannelLifecycleTarget for ApplyingFakeChannelSource {
    async fn create_channel(
        &self,
        _guild_id: &GuildId,
        create: ChannelCreate,
    ) -> Result<ChannelCreateOutcome, ManagementError> {
        self.creates.lock().unwrap().push(create.clone());
        let outcome = match self.create_outcome {
            ChannelCreateOutcome::Created(_) => {
                let mut next_id = self.next_id.lock().unwrap();
                let channel_id = ChannelId::new(*next_id);
                *next_id += 1;
                ChannelCreateOutcome::Created(channel_id)
            }
            ChannelCreateOutcome::ResponseUnknown => ChannelCreateOutcome::ResponseUnknown,
        };
        if let ChannelCreateOutcome::Created(channel_id) = outcome {
            let parent_id = create.parent_id;
            let mut catalog = self.catalog.lock().unwrap();
            catalog.channels.push(ChannelSnapshot {
                id: channel_id,
                kind: create.kind,
                manageable: true,
                name: create.name,
                parent_id,
                topic: create.topic,
                nsfw: create.nsfw,
                slowmode_seconds: create.slowmode_seconds,
                default_auto_archive_minutes: create.default_auto_archive_minutes,
                default_thread_slowmode_seconds: create.default_thread_slowmode_seconds,
                overwrites: create.overwrites,
            });
            if let Some(remove_id) = self.create_remove_channel {
                catalog.channels.retain(|channel| channel.id != remove_id);
            }
        }
        Ok(outcome)
    }

    async fn delete_channel(
        &self,
        _guild_id: &GuildId,
        channel_id: &ChannelId,
    ) -> Result<ChannelDeleteOutcome, ManagementError> {
        self.deletes.lock().unwrap().push(*channel_id);
        if self.apply_delete {
            self.catalog
                .lock()
                .unwrap()
                .channels
                .retain(|channel| channel.id != *channel_id);
        }
        Ok(self.delete_outcome)
    }
}

#[derive(Clone)]
struct UnknownChannelUpdateSource {
    catalog: Arc<Mutex<ChannelCatalog>>,
    outcome: ChannelUpdateOutcome,
    apply_update: bool,
}

impl ChannelSource for UnknownChannelUpdateSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.lock().unwrap().clone())
    }
}

impl ChannelUpdater for UnknownChannelUpdateSource {
    async fn update_channel(
        &self,
        _guild_id: &GuildId,
        channel_id: &ChannelId,
        update: ChannelUpdate,
    ) -> Result<ChannelUpdateOutcome, ManagementError> {
        if self.apply_update {
            let mut catalog = self.catalog.lock().unwrap();
            let channel = catalog
                .channels
                .iter_mut()
                .find(|channel| channel.id == *channel_id)
                .expect("更新対象 Channel がカタログに存在します");
            update.apply_to(channel);
        }
        Ok(self.outcome)
    }
}

fn lifecycle_channel_source(catalog: ChannelCatalog) -> ApplyingFakeChannelSource {
    ApplyingFakeChannelSource {
        catalog: Arc::new(Mutex::new(catalog)),
        updates: Arc::new(Mutex::new(Vec::new())),
        creates: Arc::new(Mutex::new(Vec::new())),
        deletes: Arc::new(Mutex::new(Vec::new())),
        next_id: Arc::new(Mutex::new(500)),
        create_outcome: ChannelCreateOutcome::Created(ChannelId::new(500)),
        create_remove_channel: None,
        delete_outcome: ChannelDeleteOutcome::Deleted,
        apply_delete: true,
    }
}

fn channel_snapshot(id: &str, kind: ChannelKind, name: &str, parent_id: Option<&str>) -> ChannelSnapshot {
    ChannelSnapshot {
        id: id.parse().unwrap(),
        kind,
        manageable: true,
        name: name.to_owned(),
        parent_id: parent_id.map(|id| id.parse().unwrap()),
        topic: None,
        nsfw: false,
        slowmode_seconds: 0,
        default_auto_archive_minutes: Some(1440),
        default_thread_slowmode_seconds: Some(0),
        overwrites: BTreeMap::new(),
    }
}

fn channel_state(channels: &str) -> String {
    format!(r#"{{"schema_version":1,"guild_id":"100","channels":{channels}}}"#)
}

/// Text Channel の型別属性を公開 plan 操作で差分として確認できる。
#[tokio::test]
async fn channel_plan_reports_text_attribute_changes() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![channel_snapshot("300", ChannelKind::Text, "旧ルール", None)],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
        topic = "案内"
        nsfw = true
        slowmode_seconds = 5
        default_auto_archive_minutes = 4320
        default_thread_slowmode_seconds = 10
    "#;

    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"rules":"300"}"#),
    )
    .await
    .unwrap();

    assert_eq!(plan.len(), 1);
    let attributes = plan
        .get(&ChannelLogicalId::parse("rules").unwrap())
        .and_then(ChannelChange::attributes)
        .expect("属性差分が一つの Channel 更新へまとまります");
    assert_eq!(attributes.name().unwrap().desired(), "ルール");
    assert_eq!(attributes.topic().unwrap().desired().map(String::as_str), Some("案内"));
    assert_eq!(attributes.nsfw().unwrap().desired(), &true);
    assert_eq!(attributes.slowmode_seconds().unwrap().desired(), &5);
    assert_eq!(
        attributes.default_auto_archive_minutes().unwrap().desired(),
        Some(&4320)
    );
    assert_eq!(
        attributes.default_thread_slowmode_seconds().unwrap().desired(),
        Some(&10)
    );
}

/// 先行する Channel 作成後に後続更新の対象が消えた場合も、作成済み mapping を返して停止する。
#[tokio::test]
async fn mixed_channel_apply_returns_confirmed_create_when_later_update_target_is_missing() {
    let mut source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "更新前", None)],
    });
    source.create_remove_channel = Some(ChannelId::new(300));
    let definition = r#"
        schema_version = 1
        [channels.a_create]
        type = "text"
        name = "新規"
        [channels.b_update]
        type = "text"
        name = "更新後"
    "#;
    let state_json = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "channels": {"b_update": "300"}
    }"#;
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        state_json,
    )
    .await
    .unwrap();
    assert!(matches!(
        plan.get(&ChannelLogicalId::parse("a_create").unwrap()),
        Some(ChannelChange::Create)
    ));
    assert!(
        plan.get(&ChannelLogicalId::parse("b_update").unwrap())
            .is_some_and(ChannelChange::is_update)
    );

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(
        matches!(result.status, ChannelApplyStatus::Failed(message) if message.contains("b_update") && message.contains("存在しません"))
    );
    assert!(matches!(
        result.applied.get(&ChannelLogicalId::parse("a_create").unwrap()),
        Some(ChannelChange::Create)
    ));
    assert!(
        result
            .pending
            .get(&ChannelLogicalId::parse("b_update").unwrap())
            .is_some_and(ChannelChange::is_update)
    );
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["channels"]["a_create"], "500");
    assert_eq!(returned_state["channels"]["b_update"], "300");
}

/// Category 作成後の一時的に古い catalog で子 Channel の親解決に失敗しても、親の mapping を返して停止する。
#[tokio::test]
async fn stale_catalog_after_category_creation_returns_confirmed_parent_mapping() {
    let mut source = lifecycle_channel_source(ChannelCatalog { channels: Vec::new() });
    source.create_remove_channel = Some(ChannelId::new(500));
    let definition = r#"
        schema_version = 1
        [channels.a_parent]
        type = "category"
        name = "親"
        [channels.b_child]
        type = "text"
        name = "子"
        parent = "a_parent"
    "#;
    let state_json = channel_state("{}");
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();
    assert!(matches!(
        plan.get(&ChannelLogicalId::parse("a_parent").unwrap()),
        Some(ChannelChange::Create)
    ));
    assert!(matches!(
        plan.get(&ChannelLogicalId::parse("b_child").unwrap()),
        Some(ChannelChange::Create)
    ));

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert!(matches!(
        result.status,
        ChannelApplyStatus::Failed(message) if message.contains("b_child") && message.contains("親")
    ));
    assert!(matches!(
        result.applied.get(&ChannelLogicalId::parse("a_parent").unwrap()),
        Some(ChannelChange::Create)
    ));
    assert!(matches!(
        result.pending.get(&ChannelLogicalId::parse("b_child").unwrap()),
        Some(ChannelChange::Create)
    ));
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["channels"]["a_parent"], "500");
    assert!(returned_state["channels"].get("b_child").is_none());
}

/// Category は Text 専用の nsfw 属性を公開 plan seam で拒否する。
#[tokio::test]
async fn category_rejects_nsfw_in_public_plan() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: Vec::new() },
    };
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        nsfw = true
    "#;

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state("{}"),
    )
    .await
    .expect_err("Category の nsfw は Text 専用属性として拒否されます");

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Category") && message.contains("Text 専用属性"))
    );
}

/// export した定義と state をそのまま再投入すると論理 ID と全属性が維持され、plan が空になる。
#[tokio::test]
async fn channel_export_round_trip_is_idempotent() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![
                channel_snapshot("200", ChannelKind::Category, "案内", None),
                ChannelSnapshot {
                    id: "300".parse().unwrap(),
                    kind: ChannelKind::Text,
                    manageable: true,
                    name: "ルール".to_owned(),
                    parent_id: Some("200".parse().unwrap()),
                    topic: Some("共通案内".to_owned()),
                    nsfw: true,
                    slowmode_seconds: 5,
                    default_auto_archive_minutes: Some(4320),
                    default_thread_slowmode_seconds: Some(10),
                    overwrites: BTreeMap::new(),
                },
            ],
        },
    };

    let files = export_channels(&source, guild_id(100), None).await.unwrap();
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        &files.definition_toml,
        &files.state_json,
    )
    .await
    .unwrap();

    assert!(files.definition_toml.contains("type = \"category\""));
    assert!(files.definition_toml.contains("parent = \"channel_200\""));
    assert_eq!(plan.len(), 0);
    let state: serde_json::Value = serde_json::from_str(&files.state_json).unwrap();
    assert_eq!(state["channels"]["channel_200"], "200");
    assert_eq!(state["channels"]["channel_300"], "300");
}

/// 初回 export でも Overwrite の Role/Member 対象へ決定的な論理 ID を割り当て、
/// 生成した参照宣言と state の対応を揃える。
#[tokio::test]
async fn initial_channel_export_registers_unmapped_overwrite_targets() {
    let permission = known_permission("VIEW_CHANNEL");
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![ChannelSnapshot {
                id: "300".parse().unwrap(),
                kind: ChannelKind::Text,
                manageable: true,
                name: "ルール".to_owned(),
                parent_id: None,
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: Some(0),
                overwrites: BTreeMap::from([
                    (
                        ChannelOverwriteTarget::Role(RoleId::new(400)),
                        ChannelOverwritePermissions::from_known(BTreeMap::from([(
                            permission.clone(),
                            OverwriteValue::Allow,
                        )])),
                    ),
                    (
                        ChannelOverwriteTarget::Member(MemberId::new(500)),
                        ChannelOverwritePermissions::from_known(BTreeMap::from([(permission, OverwriteValue::Deny)])),
                    ),
                ]),
            }],
        },
    };

    let files = export_channels(&source, guild_id(100), None).await.unwrap();
    let definition = parse_definition(&files.definition_toml).unwrap();
    let state: serde_json::Value = serde_json::from_str(&files.state_json).unwrap();

    assert_eq!(state["roles"]["role_400"], "400");
    assert_eq!(state["members"]["member_500"], "500");
    assert!(
        definition
            .roles
            .contains_key(&RoleLogicalId::parse("role_400").unwrap())
    );
    assert!(
        definition
            .members
            .contains_key(&MemberLogicalId::parse("member_500").unwrap())
    );
    assert!(files.definition_toml.contains("role:role_400"));
    assert!(files.definition_toml.contains("member:member_500"));
    assert!(
        files.definition_toml.contains("parent = { clear = true }"),
        "{}",
        files.definition_toml
    );
    assert!(files.definition_toml.contains("topic = { clear = true }"));
    assert!(
        files
            .definition_toml
            .contains("[channels.channel_300.overwrites.\"role:role_400\"]")
    );
    assert!(!files.definition_toml.contains("overwrites = {"));

    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        &files.definition_toml,
        &files.state_json,
    )
    .await
    .unwrap();
    assert!(plan.is_empty());
}

/// active state の Channel が catalog から消えた再 export は、対応を落とさず
/// ADR0004 の予期せぬ消失として停止する。
#[tokio::test]
async fn channel_export_reports_active_disappearance_instead_of_dropping_state() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: Vec::new() },
    };
    let error = export_channels(&source, guild_id(100), Some(&channel_state(r#"{"rules":"300"}"#)))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("予期せず消失") && message.contains("rules"))
    );
}

/// 既存 Text の parent は、論理 ID の宣言だけでなく実構成の型も Category に限定する。
#[tokio::test]
async fn existing_text_rejects_a_text_parent() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![
                channel_snapshot("300", ChannelKind::Text, "子", Some("301")),
                channel_snapshot("301", ChannelKind::Text, "親ではない", None),
            ],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.child]
        type = "text"
        parent = "parent"
        [channels.parent]
        mode = "reference"
    "#;
    let state = channel_state(r#"{"child":"300","parent":"301"}"#);

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Category")));
}

/// 新規作成の plan は論理 ID だけでなく、実際に送る希望属性を表示する。
#[tokio::test]
async fn channel_create_plan_renders_desired_attributes() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: Vec::new() },
    };
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = "information"
        topic = "案内"
        slowmode_seconds = 5
    "#;
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state("{}"),
    )
    .await
    .unwrap();

    let rendered = plan.render();
    assert!(rendered.contains("desired:"));
    assert!(rendered.contains("type: text"));
    assert!(rendered.contains("name:"));
    assert!(rendered.contains("ルール"));
    assert!(rendered.contains("parent: information"));
    assert!(!rendered.contains("TaggedLogicalId"));
    assert!(!rendered.contains("PhantomData"));
    assert!(rendered.contains("slowmode_seconds: 5"));
}

/// 新規作成の plan は default/clear を apply と同じ Discord API の具体値へ解決して表示する。
#[tokio::test]
async fn channel_create_plan_resolves_default_and_clear_values() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: Vec::new() },
    };
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
        topic = { clear = true }
        nsfw = { default = true }
        slowmode_seconds = { clear = true }
        default_auto_archive_minutes = { clear = true }
        default_thread_slowmode_seconds = { default = true }
        [channels.rules.overwrites.everyone]
        VIEW_CHANNEL = "clear"
    "#;

    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state("{}"),
    )
    .await
    .unwrap();

    let rendered = plan.render();
    assert!(rendered.contains("topic: None"));
    assert!(rendered.contains("nsfw: false"));
    assert!(rendered.contains("slowmode_seconds: 0"));
    assert!(rendered.contains("default_auto_archive_minutes: None"));
    assert!(rendered.contains("default_thread_slowmode_seconds: Some(0)"));
    assert!(!rendered.contains("Default"));
    assert!(!rendered.contains("Clear"));
}

/// Channel 作成の応答不明後は state を変更せず、所有者の bind で対応を追加できる。
#[tokio::test]
async fn binding_after_unknown_channel_creation_allows_resubmit() {
    let mut source = lifecycle_channel_source(ChannelCatalog { channels: Vec::new() });
    source.create_outcome = ChannelCreateOutcome::ResponseUnknown;
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
    "#;
    let state_json = channel_state("{}");
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();
    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(result.status, ChannelApplyStatus::CreationResponseUnknown);
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert!(returned_state["channels"].is_null());

    let bind_source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Channel,
            guild_id: guild_id(100),
        },
    };
    let bound = bind_resource(
        &bind_source,
        guild_id(100),
        definition,
        &result.state_json,
        ResourceType::Channel,
        "rules",
        "500",
    )
    .await
    .unwrap();
    let bound_state: serde_json::Value = serde_json::from_str(&bound.state_json).unwrap();
    assert_eq!(bound_state["channels"]["rules"], "500");
    assert!(
        plan_channels(
            &ChannelCatalogSource {
                catalog: ChannelCatalog {
                    channels: vec![channel_snapshot("500", ChannelKind::Text, "ルール", None)],
                },
            },
            &test_permission_vocabulary(),
            guild_id(100),
            definition,
            &bound.state_json,
        )
        .await
        .unwrap()
        .is_empty()
    );
}

/// Channel 更新の応答不明は state に残さず、別の定義を次回 plan できる。
#[tokio::test]
async fn unknown_channel_update_preserves_state_mapping_and_allows_new_definition() {
    let catalog = Arc::new(Mutex::new(ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "旧ルール", None)],
    }));
    let first_source = UnknownChannelUpdateSource {
        catalog: Arc::clone(&catalog),
        outcome: ChannelUpdateOutcome::ResponseUnknown,
        apply_update: false,
    };
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
    "#;
    let state_json = channel_state(r#"{"rules":"300"}"#);
    let plan = plan_channels(
        &first_source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();
    let lock = GuildApplyLock::default();
    let vocabulary = test_permission_vocabulary();
    let unknown = ChannelApplyWorkflow::new(&lock, &first_source, &vocabulary)
        .apply_channel_updates(
            guild_id(100),
            definition,
            &state_json,
            &plan,
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .await
        .unwrap();
    assert_eq!(unknown.status, ChannelApplyStatus::ResponseUnknown);
    let unknown_state: serde_json::Value = serde_json::from_str(&unknown.state_json).unwrap();
    let input_state: serde_json::Value = serde_json::from_str(&state_json).unwrap();
    assert_eq!(unknown_state["guild_id"], input_state["guild_id"]);
    assert_eq!(unknown_state["channels"], input_state["channels"]);

    let changed_definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "別のルール"
    "#;
    let changed_plan = plan_channels(
        &first_source,
        &test_permission_vocabulary(),
        guild_id(100),
        changed_definition,
        &unknown.state_json,
    )
    .await
    .unwrap();
    assert!(changed_plan.get(&ChannelLogicalId::parse("rules").unwrap()).is_some());

    let second_source = UnknownChannelUpdateSource {
        catalog,
        outcome: ChannelUpdateOutcome::Applied,
        apply_update: true,
    };
    let lock = GuildApplyLock::default();
    let vocabulary = test_permission_vocabulary();
    let resubmitted = ChannelApplyWorkflow::new(&lock, &second_source, &vocabulary)
        .apply_channel_updates(
            guild_id(100),
            definition,
            &unknown.state_json,
            &plan,
            Instant::now() + std::time::Duration::from_secs(60),
        )
        .await
        .unwrap();
    assert_eq!(resubmitted.status, ChannelApplyStatus::Complete);
    let resolved_state: serde_json::Value = serde_json::from_str(&resubmitted.state_json).unwrap();
    assert_eq!(resolved_state["guild_id"], input_state["guild_id"]);
    assert_eq!(resolved_state["channels"], input_state["channels"]);
    assert_eq!(second_source.catalog.lock().unwrap().channels[0].name, "ルール");
}

/// Channel 削除の応答不明時は対応表を維持し、実構成で不在を確認した次回 apply で除去する。
#[tokio::test]
async fn unknown_channel_delete_response_keeps_mapping_until_actual_absence() {
    let mut source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "不要", None)],
    });
    source.delete_outcome = ChannelDeleteOutcome::ResponseUnknown;
    source.apply_delete = false;
    let definition = "schema_version = 1\n[channels.unused]\nensure = \"absent\"\n";
    let state_json = r#"{"schema_version":1,"guild_id":"100","roles":{},"channels":{"unused":"300"}}"#.to_owned();
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();

    let unknown = apply_channels_with_options(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        ChannelApplyOptions { allow_deletions: true },
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(unknown.status, ChannelApplyStatus::DeletionResponseUnknown);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&unknown.state_json).unwrap(),
        serde_json::from_str::<serde_json::Value>(&state_json).unwrap()
    );
    assert_eq!(source.deletes.lock().unwrap().as_slice(), &[ChannelId::new(300)]);

    source.catalog.lock().unwrap().channels.clear();
    let rerun = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &unknown.state_json,
    )
    .await
    .unwrap();
    assert!(matches!(
        rerun.get(&ChannelLogicalId::parse("unused").unwrap()),
        Some(ChannelChange::Delete { discord_id }) if *discord_id == ChannelId::new(300)
    ));

    let resolved = apply_channels_with_options(
        &source,
        guild_id(100),
        definition,
        &unknown.state_json,
        &rerun,
        ChannelApplyOptions { allow_deletions: true },
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert_eq!(resolved.status, ChannelApplyStatus::Complete);
    let resolved_state: serde_json::Value = serde_json::from_str(&resolved.state_json).unwrap();
    assert!(resolved_state["channels"].get("unused").is_none());
    assert_eq!(source.deletes.lock().unwrap().as_slice(), &[ChannelId::new(300)]);
}

/// apply の can_manage_roles 照会も processing_deadline の契約に従い、期限超過時の state を返す。
#[tokio::test]
async fn channel_apply_times_out_can_manage_roles_and_returns_state() {
    let catalog = ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "旧ルール", None)],
    };
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
    "#;
    let state_json = channel_state(r#"{"rules":"300"}"#);
    let plan_source = ChannelCatalogSource {
        catalog: catalog.clone(),
    };
    let plan = plan_channels(
        &plan_source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();
    let source = BlockingCanManageRolesChannelSource { catalog };

    let started = Instant::now();
    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_millis(20),
    )
    .await
    .unwrap();

    assert!(
        started.elapsed() < Duration::from_millis(150),
        "can_manage_roles の照会が processing_deadline 後まで待機しました"
    );
    assert_eq!(result.status, ChannelApplyStatus::DeadlineExceeded);
    assert_eq!(result.pending, plan);
    let returned_state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(returned_state["guild_id"], "100");
    assert_eq!(returned_state["channels"]["rules"], "300");
    assert_eq!(returned_state["roles"], serde_json::json!({}));
}

/// Category を先に作成してから、その論理 ID を親に持つ Text Channel を作成する。
#[tokio::test]
async fn channel_apply_creates_category_before_text_child() {
    let source = lifecycle_channel_source(ChannelCatalog { channels: Vec::new() });
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = "information"
    "#;
    let state_json = channel_state("{}");
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    assert_eq!(source.creates.lock().unwrap().len(), 2);
    let creates = source.creates.lock().unwrap();
    assert_eq!(creates[0].kind, ChannelKind::Category);
    assert_eq!(creates[1].kind, ChannelKind::Text);
    assert_eq!(creates[1].parent_id, Some(ChannelId::new(500)));
    let state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(state["channels"]["information"], "500");
    assert_eq!(state["channels"]["rules"], "501");
}

/// Text 属性の更新と parent/topic の clear を適用し、再投入で差分が消える。
#[tokio::test]
async fn channel_apply_updates_attributes_and_clears_optional_values() {
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![ChannelSnapshot {
            id: "300".parse().unwrap(),
            kind: ChannelKind::Text,
            manageable: true,
            name: "旧ルール".to_owned(),
            parent_id: Some("200".parse().unwrap()),
            topic: Some("古い案内".to_owned()),
            nsfw: false,
            slowmode_seconds: 0,
            default_auto_archive_minutes: Some(1440),
            default_thread_slowmode_seconds: Some(0),
            overwrites: BTreeMap::new(),
        }],
    });
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = { clear = true }
        topic = { clear = true }
        nsfw = true
        slowmode_seconds = 10
    "#;
    let state_json = channel_state(r#"{"rules":"300"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();
    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    {
        let catalog = source.catalog.lock().unwrap();
        let channel = &catalog.channels[0];
        assert_eq!(channel.name, "ルール");
        assert_eq!(channel.parent_id, None);
        assert_eq!(channel.topic, None);
        assert!(channel.nsfw);
        assert_eq!(channel.slowmode_seconds, 10);
    }
    assert!(
        plan_channels(
            &source,
            &test_permission_vocabulary(),
            guild_id(100),
            definition,
            &result.state_json,
        )
        .await
        .unwrap()
        .is_empty()
    );
}

/// Overwrite の全権限解除は完成形の空配列として一回の PATCH で適用する。
#[tokio::test]
async fn channel_apply_deletes_permission_overwrite_when_all_permissions_are_cleared() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![ChannelSnapshot {
            id: "300".parse().unwrap(),
            kind: ChannelKind::Text,
            manageable: true,
            name: "ルール".to_owned(),
            parent_id: None,
            topic: None,
            nsfw: false,
            slowmode_seconds: 0,
            default_auto_archive_minutes: Some(1440),
            default_thread_slowmode_seconds: Some(0),
            overwrites: BTreeMap::from([(
                ChannelOverwriteTarget::Everyone,
                ChannelOverwritePermissions::from_known(BTreeMap::from([(
                    view_channel.clone(),
                    OverwriteValue::Allow,
                )])),
            )]),
        }],
    });
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        overwrites.everyone.VIEW_CHANNEL = "clear"
    "#;
    let state_json = channel_state(r#"{"rules":"300"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let updates = source.updates.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert!(updates[0].overwrites.as_ref().is_some_and(BTreeMap::is_empty));
    assert!(source.catalog.lock().unwrap().channels[0].overwrites.is_empty());
}

/// overwrite の完成形更新は、変更対象以外と SDK 未知 bit を保持し、空になった target だけを除外する。
#[tokio::test]
async fn channel_apply_preserves_unknown_bits_and_untouched_targets_in_full_overwrite_replacement() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let mut overwrites = BTreeMap::new();
    overwrites.insert(
        ChannelOverwriteTarget::Everyone,
        ChannelOverwritePermissions {
            known: BTreeMap::from([(view_channel.clone(), OverwriteValue::Allow)]),
            allow_unknown: PermissionBits::new(1_u64 << 60),
            deny_unknown: PermissionBits::default(),
        },
    );
    overwrites.insert(
        ChannelOverwriteTarget::Role(RoleId::new(400)),
        ChannelOverwritePermissions {
            known: BTreeMap::from([(view_channel.clone(), OverwriteValue::Allow)]),
            allow_unknown: PermissionBits::default(),
            deny_unknown: PermissionBits::new(1_u64 << 61),
        },
    );
    overwrites.insert(
        ChannelOverwriteTarget::Member(MemberId::new(500)),
        ChannelOverwritePermissions::from_known(BTreeMap::from([(view_channel.clone(), OverwriteValue::Allow)])),
    );
    overwrites.insert(
        ChannelOverwriteTarget::Role(RoleId::new(401)),
        ChannelOverwritePermissions {
            known: BTreeMap::new(),
            allow_unknown: PermissionBits::new(1_u64 << 62),
            deny_unknown: PermissionBits::default(),
        },
    );
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![ChannelSnapshot {
            id: "300".parse().unwrap(),
            kind: ChannelKind::Text,
            manageable: true,
            name: "ルール".to_owned(),
            parent_id: None,
            topic: None,
            nsfw: false,
            slowmode_seconds: 0,
            default_auto_archive_minutes: Some(1440),
            default_thread_slowmode_seconds: Some(0),
            overwrites,
        }],
    });
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        mode = "reference"
        [roles.observer]
        mode = "reference"
        [members.alice]
        mode = "reference"
        [channels.rules]
        type = "text"
        [channels.rules.overwrites.everyone]
        VIEW_CHANNEL = "clear"
        [channels.rules.overwrites."role:moderator"]
        VIEW_CHANNEL = "deny"
        [channels.rules.overwrites."member:alice"]
        VIEW_CHANNEL = "clear"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {"moderator": "400", "observer": "401"},
        "members": {"alice": "500"},
        "channels": {"rules": "300"}
    }"#;
    let plan = plan_channels(&source, &test_permission_vocabulary(), guild_id(100), definition, state)
        .await
        .unwrap();

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        state,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let updates = source.updates.lock().unwrap();
    assert_eq!(updates.len(), 1);
    let final_overwrites = updates[0]
        .overwrites
        .as_ref()
        .expect("permission overwrite は完成形で送信されます");
    assert_eq!(final_overwrites.len(), 3);
    let everyone = final_overwrites
        .get(&ChannelOverwriteTarget::Everyone)
        .expect("Everyone target は未知 bit があるため保持されます");
    assert!(everyone.known.is_empty());
    assert_eq!(everyone.allow_unknown, PermissionBits::new(1_u64 << 60));
    let moderator = final_overwrites
        .get(&ChannelOverwriteTarget::Role(RoleId::new(400)))
        .expect("変更対象 Role は保持されます");
    assert_eq!(moderator.known.get(&view_channel), Some(&OverwriteValue::Deny));
    assert_eq!(moderator.deny_unknown, PermissionBits::new(1_u64 << 61));
    assert_eq!(
        final_overwrites.get(&ChannelOverwriteTarget::Role(RoleId::new(401))),
        Some(&ChannelOverwritePermissions {
            known: BTreeMap::new(),
            allow_unknown: PermissionBits::new(1_u64 << 62),
            deny_unknown: PermissionBits::default(),
        })
    );
    assert!(!final_overwrites.contains_key(&ChannelOverwriteTarget::Member(MemberId::new(500))));
    assert_eq!(source.catalog.lock().unwrap().channels[0].overwrites, *final_overwrites);
}

/// Category 削除前に子 Channel の parent clear を適用し、対応表から除去する。
#[tokio::test]
async fn channel_apply_unparents_children_before_category_deletion() {
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![
            channel_snapshot("200", ChannelKind::Category, "案内", None),
            ChannelSnapshot {
                id: "300".parse().unwrap(),
                kind: ChannelKind::Text,
                manageable: true,
                name: "ルール".to_owned(),
                parent_id: Some("200".parse().unwrap()),
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: Some(0),
                overwrites: BTreeMap::new(),
            },
        ],
    });
    let definition = r#"
        schema_version = 1
        [channels.information]
        ensure = "absent"
        [channels.rules]
        type = "text"
        parent = { clear = true }
    "#;
    let state_json = channel_state(r#"{"information":"200","rules":"300"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();
    assert!(
        plan.get(&ChannelLogicalId::parse("information").unwrap())
            .is_some_and(ChannelChange::is_delete)
    );
    assert!(
        plan.get(&ChannelLogicalId::parse("rules").unwrap())
            .is_some_and(ChannelChange::is_update)
    );

    let result = apply_channels_with_options(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        ChannelApplyOptions { allow_deletions: true },
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    assert_eq!(source.deletes.lock().unwrap().as_slice(), &[ChannelId::new(200)]);
    let state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert!(state["channels"].get("information").is_none());
    assert_eq!(source.catalog.lock().unwrap().channels[0].parent_id, None);
}

/// 同じ plan 内で作成する Category を既存 Text の親として解決し、作成後に移動する。
#[tokio::test]
async fn channel_apply_moves_an_existing_text_under_a_planned_category() {
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
    });
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = "information"
    "#;
    let state_json = channel_state(r#"{"rules":"300"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();

    assert!(matches!(
        plan.get(&ChannelLogicalId::parse("information").unwrap()),
        Some(ChannelChange::Create)
    ));
    assert!(
        plan.get(&ChannelLogicalId::parse("rules").unwrap())
            .is_some_and(ChannelChange::is_update)
    );

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    assert_eq!(source.creates.lock().unwrap().len(), 1);
    assert_eq!(
        source.updates.lock().unwrap()[0].parent_id,
        ChannelUpdateValue::Set(ChannelId::new(500))
    );
    assert_eq!(
        source
            .catalog
            .lock()
            .unwrap()
            .channels
            .iter()
            .find(|channel| channel.id == ChannelId::new(300))
            .and_then(|channel| channel.parent_id),
        Some(ChannelId::new(500))
    );
}

/// reserved everyone の削除宣言を Channel overwrite の早期分岐で見落とさない。
#[tokio::test]
async fn channel_plan_rejects_an_everyone_overwrite_when_everyone_is_absent() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
        },
    };
    let definition = r#"
        schema_version = 1
        [roles.everyone]
        ensure = "absent"
        [channels.rules]
        type = "text"
        [channels.rules.overwrites.everyone]
        VIEW_CHANNEL = "allow"
    "#;
    let state = channel_state(r#"{"rules":"300"}"#);

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state,
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Role everyone") && message.contains("削除宣言"))
    );
}

/// 対応がない Category を作成し、後続の子 Channel 作成へ新しい親を渡す。
#[tokio::test]
async fn channel_plan_and_apply_recreate_category_before_creating_a_child() {
    let source = lifecycle_channel_source(ChannelCatalog { channels: Vec::new() });
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = "information"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "channels": {}
    }"#;
    let plan = plan_channels(&source, &test_permission_vocabulary(), guild_id(100), definition, state)
        .await
        .unwrap();

    assert!(matches!(
        plan.get(&ChannelLogicalId::parse("information").unwrap()),
        Some(ChannelChange::Create)
    ));
    assert!(matches!(
        plan.get(&ChannelLogicalId::parse("rules").unwrap()),
        Some(ChannelChange::Create)
    ));

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        state,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let creates = source.creates.lock().unwrap();
    assert_eq!(creates.len(), 2);
    assert_eq!(creates[0].kind, ChannelKind::Category);
    assert_eq!(creates[1].kind, ChannelKind::Text);
    assert_eq!(creates[1].parent_id, Some(ChannelId::new(500)));
    let state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(state["channels"]["information"], "500");
    assert_eq!(state["channels"]["rules"], "501");
}

/// 対応がない Category を作成し、既存の子 Channel を新しい親へ移動する。
#[tokio::test]
async fn channel_plan_and_apply_recreate_category_before_moving_a_child() {
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
    });
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = "information"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "channels": {"rules": "300"}
    }"#;
    let plan = plan_channels(&source, &test_permission_vocabulary(), guild_id(100), definition, state)
        .await
        .unwrap();

    assert!(matches!(
        plan.get(&ChannelLogicalId::parse("information").unwrap()),
        Some(ChannelChange::Create)
    ));
    let rules_change = plan
        .get(&ChannelLogicalId::parse("rules").unwrap())
        .expect("子 Channel の移動 plan が必要です");
    assert!(rules_change.is_update());
    assert!(plan.render().contains("parent: None -> planned:information"));

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        state,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    assert_eq!(source.creates.lock().unwrap().len(), 1);
    assert_eq!(source.updates.lock().unwrap().len(), 1);
    assert_eq!(
        source.updates.lock().unwrap()[0].parent_id,
        ChannelUpdateValue::Set(ChannelId::new(500))
    );
    assert_eq!(
        source
            .catalog
            .lock()
            .unwrap()
            .channels
            .iter()
            .find(|channel| channel.id == ChannelId::new(300))
            .and_then(|channel| channel.parent_id),
        Some(ChannelId::new(500))
    );
    let state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert_eq!(state["channels"]["information"], "500");
    assert_eq!(state["channels"]["rules"], "300");
}

/// Voice/Forum 等の未管理 Channel も catalog に残し、Category 削除時に見落とさない。
#[tokio::test]
async fn category_deletion_rejects_an_unsupported_child_from_catalog() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![
                channel_snapshot("200", ChannelKind::Category, "案内", None),
                channel_snapshot("400", ChannelKind::Unsupported, "通話", Some("200")),
            ],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.information]
        ensure = "absent"
    "#;
    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"information":"200"}"#),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("400") && message.contains("管理外"))
    );
}

/// Discord の name/topic/slowmode 型制約を definition parse 時点で診断する。
#[test]
fn channel_definition_rejects_discord_field_limits() {
    let name = "a".repeat(101);
    let error = parse_definition(&format!(
        "schema_version = 1\n[channels.rules]\ntype = \"text\"\nname = \"{name}\"\n"
    ))
    .unwrap_err();
    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("name") && message.contains("100"))
    );

    let topic = "a".repeat(1025);
    let error = parse_definition(&format!(
        "schema_version = 1\n[channels.rules]\ntype = \"text\"\nname = \"rules\"\ntopic = \"{topic}\"\n"
    ))
    .unwrap_err();
    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("topic") && message.contains("1024"))
    );

    let topic = "a".repeat(1024);
    parse_definition(&format!(
        "schema_version = 1\n[channels.rules]\ntype = \"text\"\nname = \"rules\"\ntopic = \"{topic}\"\n"
    ))
    .expect("Discord の Text topic 上限ちょうどは受け付けます");

    for attribute in ["slowmode_seconds", "default_thread_slowmode_seconds"] {
        let error = parse_definition(&format!(
            "schema_version = 1\n[channels.rules]\ntype = \"text\"\nname = \"rules\"\n{attribute} = 21601\n"
        ))
        .unwrap_err();
        assert!(
            matches!(error, ManagementError::InvalidDefinition(message) if message.contains(attribute) && message.contains("21600"))
        );
    }
}

/// permission overwrite の差分は MANAGE_ROLES がない状態で plan を拒否する。
#[tokio::test]
async fn channel_plan_rejects_overwrite_updates_without_manage_roles() {
    let source = PermissionAwareChannelSource {
        catalog: ChannelCatalog {
            channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
        },
        can_manage_roles: false,
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        mode = "reference"
        [channels.rules]
        type = "text"
        [channels.rules.overwrites."role:moderator"]
        VIEW_CHANNEL = "allow"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {"moderator": "400"},
        "channels": {"rules": "300"}
    }"#;

    let error = plan_channels(&source, &test_permission_vocabulary(), guild_id(100), definition, state)
        .await
        .unwrap_err();
    assert!(matches!(error, ManagementError::ChannelPermissionDenied(message) if message.contains("MANAGE_ROLES")));
}

/// 定義から Channel を外す管理解除で state から対応が除かれる。
#[tokio::test]
async fn deleted_channel_mapping_is_released_when_definition_is_omitted() {
    let source = lifecycle_channel_source(ChannelCatalog { channels: Vec::new() });
    let state_json = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "channels": {"old": "300"}
    }"#;
    let definition = "schema_version = 1\n";
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        state_json,
    )
    .await
    .unwrap();
    assert!(
        plan.get(&ChannelLogicalId::parse("old").unwrap())
            .is_some_and(|change| matches!(change, ChannelChange::Release { .. }))
    );

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        state_json,
        &plan,
        Instant::now() + std::time::Duration::from_secs(60),
    )
    .await
    .unwrap();
    let state: serde_json::Value = serde_json::from_str(&result.state_json).unwrap();
    assert!(state.get("channels").is_none());
}
