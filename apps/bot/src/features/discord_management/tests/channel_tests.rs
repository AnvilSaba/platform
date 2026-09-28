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

#[derive(Clone)]
struct FeaturelessChannelSource;

impl ChannelSource for FeaturelessChannelSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(ChannelCatalog { channels: Vec::new() })
    }

    async fn supports_announcement_channels(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(false)
    }

    async fn validate_channel_permission_targets(
        &self,
        _guild_id: &GuildId,
        _role_ids: &[RoleId],
        _member_ids: &[MemberId],
    ) -> Result<(), ManagementError> {
        Ok(())
    }

    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }
}

impl ChannelSource for ChannelCatalogSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.clone())
    }

    async fn supports_announcement_channels(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }

    async fn validate_channel_permission_targets(
        &self,
        _guild_id: &GuildId,
        _role_ids: &[RoleId],
        _member_ids: &[MemberId],
    ) -> Result<(), ManagementError> {
        Ok(())
    }

    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }
}

#[derive(Clone)]
struct PermissionAwareChannelSource {
    catalog: ChannelCatalog,
    can_manage_roles: bool,
}

#[derive(Clone)]
struct RejectingChannelReferenceSource {
    catalog: ChannelCatalog,
}

impl ChannelSource for RejectingChannelReferenceSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.clone())
    }

    async fn supports_announcement_channels(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }

    async fn validate_channel_permission_targets(
        &self,
        _guild_id: &GuildId,
        _role_ids: &[RoleId],
        _member_ids: &[MemberId],
    ) -> Result<(), ManagementError> {
        Err(ManagementError::InvalidState(
            "権限対象 Member の Guild 所属を確認できません".to_owned(),
        ))
    }

    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }
}

impl ChannelUpdater for RejectingChannelReferenceSource {
    async fn update_channel(
        &self,
        _guild_id: &GuildId,
        _channel_id: &ChannelId,
        _update: ChannelUpdate,
    ) -> Result<ChannelUpdateOutcome, ManagementError> {
        unreachable!("Channel overwrite の実在検証を通過した場合だけ更新へ進みます")
    }
}

impl ChannelSource for PermissionAwareChannelSource {
    async fn channel_catalog(&self, _guild_id: &GuildId) -> Result<ChannelCatalog, ManagementError> {
        Ok(self.catalog.clone())
    }

    async fn supports_announcement_channels(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }

    async fn validate_channel_permission_targets(
        &self,
        _guild_id: &GuildId,
        _role_ids: &[RoleId],
        _member_ids: &[MemberId],
    ) -> Result<(), ManagementError> {
        Ok(())
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

    async fn supports_announcement_channels(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }

    async fn validate_channel_permission_targets(
        &self,
        _guild_id: &GuildId,
        _role_ids: &[RoleId],
        _member_ids: &[MemberId],
    ) -> Result<(), ManagementError> {
        Ok(())
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

impl ChannelPositionUpdater for BlockingCanManageRolesChannelSource {
    async fn update_channel_positions(
        &self,
        _guild_id: &GuildId,
        _updates: Vec<ChannelPositionUpdate>,
    ) -> Result<ChannelPositionUpdateOutcome, ManagementError> {
        unreachable!("can_manage_roles の期限超過後に Channel 位置更新へ進みません")
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
    position_updates: Arc<Mutex<Vec<Vec<ChannelPositionUpdate>>>>,
    events: Arc<Mutex<Vec<String>>>,
    position_outcome: ChannelPositionUpdateOutcome,
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

    async fn supports_announcement_channels(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }

    async fn validate_channel_permission_targets(
        &self,
        _guild_id: &GuildId,
        _role_ids: &[RoleId],
        _member_ids: &[MemberId],
    ) -> Result<(), ManagementError> {
        Ok(())
    }

    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }
}

impl ChannelUpdater for ApplyingFakeChannelSource {
    async fn update_channel(
        &self,
        _guild_id: &GuildId,
        channel_id: &ChannelId,
        update: ChannelUpdate,
    ) -> Result<ChannelUpdateOutcome, ManagementError> {
        self.events.lock().unwrap().push(format!("update:{channel_id}"));
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

impl ChannelPositionUpdater for ApplyingFakeChannelSource {
    async fn update_channel_positions(
        &self,
        _guild_id: &GuildId,
        updates: Vec<ChannelPositionUpdate>,
    ) -> Result<ChannelPositionUpdateOutcome, ManagementError> {
        self.events.lock().unwrap().push("positions".to_owned());
        self.position_updates.lock().unwrap().push(updates.clone());
        let mut catalog = self.catalog.lock().unwrap();
        for update in updates {
            let channel = catalog
                .channels
                .iter_mut()
                .find(|channel| channel.id == update.channel_id)
                .expect("位置更新対象 Channel がカタログに存在します");
            channel.position = u16::try_from(update.position).expect("テスト位置は u16 に収まります");
        }
        Ok(self.position_outcome)
    }
}

impl ChannelLifecycleTarget for ApplyingFakeChannelSource {
    async fn create_channel(
        &self,
        _guild_id: &GuildId,
        create: ChannelCreate,
    ) -> Result<ChannelCreateOutcome, ManagementError> {
        self.events.lock().unwrap().push(format!("create:{}", create.name));
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
                position: 0,
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
        self.events.lock().unwrap().push(format!("delete:{channel_id}"));
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

    async fn supports_announcement_channels(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
    }

    async fn validate_channel_permission_targets(
        &self,
        _guild_id: &GuildId,
        _role_ids: &[RoleId],
        _member_ids: &[MemberId],
    ) -> Result<(), ManagementError> {
        Ok(())
    }

    async fn can_manage_roles(&self, _guild_id: &GuildId) -> Result<bool, ManagementError> {
        Ok(true)
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
        position_updates: Arc::new(Mutex::new(Vec::new())),
        events: Arc::new(Mutex::new(Vec::new())),
        position_outcome: ChannelPositionUpdateOutcome::Applied,
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
        position: 0,
        kind,
        manageable: true,
        name: name.to_owned(),
        parent_id: parent_id.map(|id| id.parse().unwrap()),
        topic: None,
        nsfw: false,
        slowmode_seconds: 0,
        default_auto_archive_minutes: Some(1440),
        default_thread_slowmode_seconds: 0,
        overwrites: BTreeMap::new(),
    }
}

fn channel_state(channels: &str) -> String {
    format!(r#"{{"schema_version":1,"guild_id":"100","channels":{channels}}}"#)
}

/// 設定セットは左から右、最後に直接指定を合成し、Overwrite は権限名単位で保持する。
/// マーカーによる置換後の最終値が実構成と同じなら、設定セット名や共通化方法は差分にしない。
#[tokio::test]
async fn channel_settings_sets_resolve_to_the_same_final_state_without_changes() {
    let mut actual = channel_snapshot("300", ChannelKind::Text, "ルール", None);
    actual.overwrites.insert(
        ChannelOverwriteTarget::Everyone,
        ChannelOverwritePermissions::from_known(BTreeMap::from([
            (known_permission("VIEW_CHANNEL"), OverwriteValue::Deny),
            (known_permission("SEND_MESSAGES"), OverwriteValue::Allow),
        ])),
    );
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: vec![actual] },
    };
    let definition = r#"
        schema_version = 1

        [settings_sets.channel.base]
        type = "text"
        topic = "置換前"
        [settings_sets.channel.base.overwrites.everyone]
        VIEW_CHANNEL = "allow"
        SEND_MESSAGES = "deny"

        [settings_sets.channel.renamed_override]
        topic = { clear = true }
        [settings_sets.channel.renamed_override.overwrites.everyone]
        SEND_MESSAGES = "allow"

        [channels.rules]
        settings_sets = ["base", "renamed_override"]
        name = "ルール"
        [channels.rules.overwrites.everyone]
        VIEW_CHANNEL = "deny"
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

    assert!(plan.is_empty());
}

/// 設定セットの Channel 型は後勝ちで隠さず、対象 Channel の型との不一致を診断する。
#[tokio::test]
async fn channel_rejects_a_settings_set_with_a_different_kind() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: Vec::new() },
    };
    let definition = r#"
        schema_version = 1

        [settings_sets.channel.category_defaults]
        type = "category"

        [channels.rules]
        settings_sets = ["category_defaults"]
        type = "text"
        name = "ルール"
    "#;

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state("{}"),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("category_defaults") && message.contains("type"))
    );
}

/// 複数の設定セットが互いに異なる Channel 型を宣言する場合も、合成前に診断する。
#[tokio::test]
async fn channel_rejects_settings_sets_with_different_kinds() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: Vec::new() },
    };
    let definition = r#"
        schema_version = 1

        [settings_sets.channel.category_defaults]
        type = "category"

        [settings_sets.channel.text_defaults]
        type = "text"

        [channels.rules]
        settings_sets = ["category_defaults", "text_defaults"]
        name = "ルール"
    "#;

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state("{}"),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("category_defaults") && message.contains("text_defaults") && message.contains("type"))
    );
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
    assert_eq!(attributes.default_thread_slowmode_seconds().unwrap().desired(), &10);
}

/// Category と親ごとの子 Channel の順序を、未管理 Channel を挟んだ兄弟全体の
/// 位置更新計画として確認できる。
#[tokio::test]
async fn channel_plan_reports_category_and_child_order_changes() {
    let mut category_a = channel_snapshot("200", ChannelKind::Category, "A", None);
    category_a.position = 2;
    let mut category_b = channel_snapshot("300", ChannelKind::Category, "B", None);
    category_b.position = 0;
    let mut unmanaged_first = channel_snapshot("400", ChannelKind::Text, "未管理1", None);
    unmanaged_first.position = 1;
    let mut unmanaged_second = channel_snapshot("401", ChannelKind::Text, "未管理2", None);
    unmanaged_second.position = 3;
    let mut child_first = channel_snapshot("500", ChannelKind::Text, "子1", Some("200"));
    child_first.position = 0;
    let mut child_second = channel_snapshot("600", ChannelKind::Text, "子2", Some("200"));
    child_second.position = 1;
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![
                category_a,
                category_b,
                unmanaged_first,
                unmanaged_second,
                child_first,
                child_second,
            ],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.category_a]
        type = "category"
        name = "A"
        [channels.category_b]
        type = "category"
        name = "B"
        [channels.child_first]
        type = "text"
        name = "子1"
        parent = "category_a"
        [channels.child_second]
        type = "text"
        name = "子2"
        parent = "category_a"
        [order]
        categories = ["category_a", "category_b"]
        [order.children]
        category_a = ["child_second", "child_first"]
    "#;
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"category_a":"200","category_b":"300","child_first":"500","child_second":"600"}"#),
    )
    .await
    .unwrap();

    let order = plan
        .order()
        .expect("Category と子 Channel の順序変更が plan に含まれます");
    assert_eq!(order.groups().len(), 2);
    assert_eq!(
        order.groups()[0].expected_order(),
        &[
            ChannelId::new(400),
            ChannelId::new(200),
            ChannelId::new(300),
            ChannelId::new(401)
        ]
    );
    assert_eq!(
        order.groups()[0]
            .updates()
            .iter()
            .map(|update| update.channel_id)
            .collect::<Vec<_>>(),
        vec![
            ChannelId::new(400),
            ChannelId::new(200),
            ChannelId::new(300),
            ChannelId::new(401)
        ]
    );
    assert_eq!(
        order.groups()[1].expected_order(),
        &[ChannelId::new(600), ChannelId::new(500)]
    );
}

/// Category に属さない Text Channel の相対順序を、top-level 全体の位置更新として計画する。
#[tokio::test]
async fn channel_plan_reports_uncategorized_order_changes() {
    let mut channel_a = channel_snapshot("400", ChannelKind::Text, "A", None);
    channel_a.position = 0;
    let mut category = channel_snapshot("200", ChannelKind::Category, "区切り", None);
    category.position = 1;
    let mut channel_b = channel_snapshot("401", ChannelKind::Text, "B", None);
    channel_b.position = 2;
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![channel_a, category, channel_b],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.channel_a]
        type = "text"
        name = "A"
        parent = { clear = true }
        [channels.channel_b]
        type = "text"
        name = "B"
        parent = { clear = true }
        [order]
        uncategorized = ["channel_b", "channel_a"]
    "#;

    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"channel_a":"400","channel_b":"401"}"#),
    )
    .await
    .unwrap();

    let order = plan.order().expect("親なし Channel の順序変更が plan に含まれます");
    assert_eq!(order.groups().len(), 1);
    assert_eq!(
        order.groups()[0].expected_order(),
        &[ChannelId::new(200), ChannelId::new(401), ChannelId::new(400)]
    );
}

/// 未作成 managed Channel を含む固定 anchor 跨ぎを、作成前に診断する。
#[tokio::test]
async fn channel_order_rejects_uncreated_channel_crossing_fixed_anchor_before_create() {
    let destination = channel_snapshot("200", ChannelKind::Category, "移動先", None);
    let mut anchor = channel_snapshot("300", ChannelKind::Text, "固定 anchor", Some("200"));
    anchor.manageable = false;
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![destination, anchor],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.destination]
        type = "category"
        name = "移動先"
        [channels.anchor]
        mode = "reference"
        [channels.new_channel]
        type = "text"
        name = "新規"
        parent = "destination"
        [order.children]
        destination = ["new_channel", "anchor"]
    "#;

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"destination":"200","anchor":"300"}"#),
    )
    .await
    .expect_err("未作成 Channel が固定 anchor を越える順序は作成前に拒否します");
    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("固定位置")));
}

/// 未作成 Category の子順序に、現在は別親にいる参照専用 Channel を列挙できない。
#[tokio::test]
async fn channel_order_rejects_reference_child_under_uncreated_category_before_create() {
    let other_category = channel_snapshot("200", ChannelKind::Category, "現在の親", None);
    let anchor = channel_snapshot("300", ChannelKind::Text, "固定子", Some("200"));
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![other_category, anchor],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.destination]
        type = "category"
        name = "新カテゴリ"
        [channels.anchor]
        mode = "reference"
        [order.children]
        destination = ["anchor"]
    "#;

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"anchor":"300"}"#),
    )
    .await
    .expect_err("未作成 Category の固定子を別親から移動する順序は作成前に拒否します");
    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未作成 Category") && message.contains("参照専用"))
    );
}

/// 他の変更後に Channel の専用位置 API を一括実行し、再取得した兄弟順を確認する。
#[tokio::test]
async fn apply_updates_channel_positions_after_other_changes() {
    let mut category_a = channel_snapshot("200", ChannelKind::Category, "A", None);
    category_a.position = 1;
    let mut category_b = channel_snapshot("300", ChannelKind::Category, "B", None);
    category_b.position = 0;
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![category_a, category_b],
    });
    let definition = r#"
        schema_version = 1
        [channels.category_a]
        type = "category"
        name = "A"
        [channels.category_b]
        type = "category"
        name = "B"
        [order]
        categories = ["category_a", "category_b"]
    "#;
    let state_json = channel_state(r#"{"category_a":"200","category_b":"300"}"#);
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
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let updates = source.position_updates.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0].iter().map(|update| update.channel_id).collect::<Vec<_>>(),
        vec![ChannelId::new(200), ChannelId::new(300)]
    );
    let catalog = source.catalog.lock().unwrap();
    let category_a_position = catalog
        .channels
        .iter()
        .find(|channel| channel.id == ChannelId::new(200))
        .unwrap()
        .position;
    let category_b_position = catalog
        .channels
        .iter()
        .find(|channel| channel.id == ChannelId::new(300))
        .unwrap()
        .position;
    assert!(category_a_position < category_b_position);
}

/// 既存 Text Channel の親変更後に、移動先 sibling 集合から相対順序を再計画する。
#[tokio::test]
async fn apply_replans_channel_order_after_parent_move() {
    let destination = channel_snapshot("200", ChannelKind::Category, "移動先", None);
    let mut moved = channel_snapshot("300", ChannelKind::Text, "移動する", None);
    moved.position = 0;
    let mut sibling = channel_snapshot("400", ChannelKind::Text, "既存の子", Some("200"));
    sibling.position = 1;
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![destination, moved, sibling],
    });
    let definition = r#"
        schema_version = 1
        [channels.destination]
        type = "category"
        name = "移動先"
        [channels.moved]
        type = "text"
        name = "移動する"
        parent = "destination"
        [channels.sibling]
        type = "text"
        name = "既存の子"
        parent = "destination"
        [order.children]
        destination = ["sibling", "moved"]
    "#;
    let state_json = channel_state(r#"{"destination":"200","moved":"300","sibling":"400"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();
    assert_eq!(
        plan.order().unwrap().groups()[0].expected_order(),
        &[ChannelId::new(400), ChannelId::new(300)]
    );

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

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    assert_eq!(
        source.position_updates.lock().unwrap()[0]
            .iter()
            .map(|update| update.channel_id)
            .collect::<Vec<_>>(),
        vec![ChannelId::new(400), ChannelId::new(300)]
    );
    assert_eq!(
        source
            .catalog
            .lock()
            .unwrap()
            .channels
            .iter()
            .find(|channel| channel.id == ChannelId::new(300))
            .unwrap()
            .parent_id,
        Some(ChannelId::new(200))
    );
}

/// Category と複数親の子 Channel の位置更新を、一つの batch payload にまとめる。
#[tokio::test]
async fn apply_batches_channel_positions_across_sibling_groups() {
    let mut category_a = channel_snapshot("200", ChannelKind::Category, "A", None);
    category_a.position = 1;
    let mut category_b = channel_snapshot("300", ChannelKind::Category, "B", None);
    category_b.position = 0;
    let mut child_a_first = channel_snapshot("400", ChannelKind::Text, "A1", Some("200"));
    child_a_first.position = 0;
    let mut child_a_second = channel_snapshot("401", ChannelKind::Text, "A2", Some("200"));
    child_a_second.position = 1;
    let mut child_b_first = channel_snapshot("500", ChannelKind::Text, "B1", Some("300"));
    child_b_first.position = 0;
    let mut child_b_second = channel_snapshot("501", ChannelKind::Text, "B2", Some("300"));
    child_b_second.position = 1;
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![
            category_a,
            category_b,
            child_a_first,
            child_a_second,
            child_b_first,
            child_b_second,
        ],
    });
    let definition = r#"
        schema_version = 1
        [channels.category_a]
        type = "category"
        name = "A"
        [channels.category_b]
        type = "category"
        name = "B"
        [channels.child_a_first]
        type = "text"
        name = "A1"
        parent = "category_a"
        [channels.child_a_second]
        type = "text"
        name = "A2"
        parent = "category_a"
        [channels.child_b_first]
        type = "text"
        name = "B1"
        parent = "category_b"
        [channels.child_b_second]
        type = "text"
        name = "B2"
        parent = "category_b"
        [order]
        categories = ["category_a", "category_b"]
        [order.children]
        category_a = ["child_a_second", "child_a_first"]
        category_b = ["child_b_second", "child_b_first"]
    "#;
    let state_json = channel_state(
        r#"{"category_a":"200","category_b":"300","child_a_first":"400","child_a_second":"401","child_b_first":"500","child_b_second":"501"}"#,
    );
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
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let position_updates = source.position_updates.lock().unwrap();
    assert_eq!(position_updates.len(), 1);
    assert_eq!(
        position_updates[0]
            .iter()
            .map(|update| (update.channel_id, update.position))
            .collect::<Vec<_>>(),
        vec![
            (ChannelId::new(200), 0),
            (ChannelId::new(300), 1),
            (ChannelId::new(401), 0),
            (ChannelId::new(400), 1),
            (ChannelId::new(501), 0),
            (ChannelId::new(500), 1),
        ]
    );
}

/// lifecycle／属性変更後に位置 batch を最後に実行し、成功後の再適用を無差分にする。
#[tokio::test]
async fn channel_apply_orders_all_changes_before_final_position_batch_and_is_idempotent() {
    let destination = channel_snapshot("200", ChannelKind::Category, "旧移動先", None);
    let mut moved = channel_snapshot("300", ChannelKind::Text, "旧チャンネル", None);
    moved.position = 0;
    let mut sibling = channel_snapshot("400", ChannelKind::Text, "兄弟", Some("200"));
    sibling.position = 1;
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![destination, moved, sibling],
    });
    let definition = r#"
        schema_version = 1
        [channels.destination]
        type = "category"
        name = "新移動先"
        [channels.moved]
        type = "text"
        name = "新チャンネル"
        parent = "destination"
        [channels.sibling]
        type = "text"
        name = "兄弟"
        parent = "destination"
        [channels.new_channel]
        type = "text"
        name = "新規"
        parent = "destination"
        [order.children]
        destination = ["sibling", "new_channel", "moved"]
    "#;
    let state_json = channel_state(r#"{"destination":"200","moved":"300","sibling":"400"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state_json,
    )
    .await
    .unwrap();

    let first = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state_json,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(first.status, ChannelApplyStatus::Complete);
    assert_eq!(
        *source.events.lock().unwrap(),
        vec![
            "update:200".to_owned(),
            "create:新規".to_owned(),
            "update:300".to_owned(),
            "positions".to_owned(),
        ]
    );
    let event_count = source.events.lock().unwrap().len();

    let second_plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &first.state_json,
    )
    .await
    .unwrap();
    assert!(second_plan.is_empty(), "成功後の再計画は無差分であるべきです");
    let second = apply_channels(
        &source,
        guild_id(100),
        definition,
        &first.state_json,
        &second_plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(second.status, ChannelApplyStatus::Complete);
    assert_eq!(source.events.lock().unwrap().len(), event_count);
}

/// Channel 位置 API の応答が不明でも、再取得した兄弟順が希望値なら成功として確定する。
#[tokio::test]
async fn unknown_channel_position_response_is_confirmed_by_refetch() {
    let mut category_a = channel_snapshot("200", ChannelKind::Category, "A", None);
    category_a.position = 1;
    let mut category_b = channel_snapshot("300", ChannelKind::Category, "B", None);
    category_b.position = 0;
    let mut source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![category_a, category_b],
    });
    source.position_outcome = ChannelPositionUpdateOutcome::ResponseUnknown;
    let definition = r#"
        schema_version = 1
        [channels.category_a]
        type = "category"
        name = "A"
        [channels.category_b]
        type = "category"
        name = "B"
        [order]
        categories = ["category_a", "category_b"]
    "#;
    let state_json = channel_state(r#"{"category_a":"200","category_b":"300"}"#);
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
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
}

/// Channel の参照専用 Category を固定 anchor として使い、兄弟全体を送る場合も
/// anchor 自身の位置を変更しない。
#[tokio::test]
async fn channel_plan_keeps_reference_category_at_its_anchor_position() {
    let mut anchor = channel_snapshot("250", ChannelKind::Category, "基準", None);
    anchor.position = 0;
    let mut first = channel_snapshot("200", ChannelKind::Category, "A", None);
    first.position = 1;
    let mut second = channel_snapshot("300", ChannelKind::Category, "B", None);
    second.position = 2;
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![anchor, first, second],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.anchor]
        mode = "reference"
        [channels.first]
        type = "category"
        name = "A"
        [channels.second]
        type = "category"
        name = "B"
        [order]
        categories = ["anchor", "second", "first"]
    "#;
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"anchor":"250","first":"200","second":"300"}"#),
    )
    .await
    .unwrap();

    let updates = &plan.order().unwrap().groups()[0].updates();
    assert_eq!(
        updates.iter().map(|update| update.channel_id).collect::<Vec<_>>(),
        vec![ChannelId::new(250), ChannelId::new(300), ChannelId::new(200)]
    );
    assert_eq!(updates[0].position, 0, "参照専用 anchor は直接移動しません");
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
                    position: 0,
                    kind: ChannelKind::Text,
                    manageable: true,
                    name: "ルール".to_owned(),
                    parent_id: Some("200".parse().unwrap()),
                    topic: Some("共通案内".to_owned()),
                    nsfw: true,
                    slowmode_seconds: 5,
                    default_auto_archive_minutes: Some(4320),
                    default_thread_slowmode_seconds: 10,
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

/// Channel export が Category と親ごとの子 Channel の UI 順序を出力し、再投入しても
/// 無差分になることを保証する。
#[tokio::test]
async fn channel_export_order_round_trips_into_an_empty_plan() {
    let mut category_a = channel_snapshot("200", ChannelKind::Category, "A", None);
    category_a.position = 1;
    let mut category_b = channel_snapshot("300", ChannelKind::Category, "B", None);
    category_b.position = 0;
    let mut child_a = channel_snapshot("400", ChannelKind::Text, "子A", Some("200"));
    child_a.position = 0;
    let mut child_b = channel_snapshot("401", ChannelKind::Text, "子B", Some("200"));
    child_b.position = 1;
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![category_a, category_b, child_a, child_b],
        },
    };

    let exported = export_channels(&source, guild_id(100), None).await.unwrap();
    let definition = parse_definition(&exported.definition_toml).unwrap();
    let order = definition.order.as_ref().unwrap();
    assert_eq!(
        order.categories,
        vec![
            ChannelLogicalId::parse("channel_300").unwrap(),
            ChannelLogicalId::parse("channel_200").unwrap()
        ]
    );
    assert_eq!(
        order.children.get(&ChannelLogicalId::parse("channel_200").unwrap()),
        Some(&vec![
            ChannelLogicalId::parse("channel_400").unwrap(),
            ChannelLogicalId::parse("channel_401").unwrap(),
        ])
    );
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        &exported.definition_toml,
        &exported.state_json,
    )
    .await
    .unwrap();
    assert!(plan.is_empty(), "export 結果を再投入した plan は無差分であるべきです");
}

/// Category に属さない Text Channel の UI 順序も export し、再投入時に維持する。
#[tokio::test]
async fn channel_export_order_includes_uncategorized_channels() {
    let mut channel_a = channel_snapshot("400", ChannelKind::Text, "A", None);
    channel_a.position = 1;
    let mut channel_b = channel_snapshot("401", ChannelKind::Text, "B", None);
    channel_b.position = 0;
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![channel_a, channel_b],
        },
    };

    let exported = export_channels(&source, guild_id(100), None).await.unwrap();
    let definition = parse_definition(&exported.definition_toml).unwrap();
    assert_eq!(
        definition.order.as_ref().unwrap().uncategorized,
        vec![
            ChannelLogicalId::parse("channel_401").unwrap(),
            ChannelLogicalId::parse("channel_400").unwrap(),
        ],
        "Category に属さない Channel の順序が export されるべきです"
    );
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        &exported.definition_toml,
        &exported.state_json,
    )
    .await
    .unwrap();
    assert!(plan.is_empty(), "export 結果を再投入した plan は無差分であるべきです");
}

/// Category と同じ overwrite を持つ子 Channel は、export で同期指定として表現する。
#[tokio::test]
async fn channel_export_preserves_category_permission_sync() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let overwrites = BTreeMap::from([(
        ChannelOverwriteTarget::Everyone,
        ChannelOverwritePermissions::from_known(BTreeMap::from([(view_channel, OverwriteValue::Deny)])),
    )]);
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![
                ChannelSnapshot {
                    id: "200".parse().unwrap(),
                    position: 0,
                    kind: ChannelKind::Category,
                    manageable: true,
                    name: "非公開".to_owned(),
                    parent_id: None,
                    topic: None,
                    nsfw: false,
                    slowmode_seconds: 0,
                    default_auto_archive_minutes: Some(1440),
                    default_thread_slowmode_seconds: 0,
                    overwrites: overwrites.clone(),
                },
                ChannelSnapshot {
                    id: "300".parse().unwrap(),
                    position: 0,
                    kind: ChannelKind::Text,
                    manageable: true,
                    name: "運営".to_owned(),
                    parent_id: Some("200".parse().unwrap()),
                    topic: None,
                    nsfw: false,
                    slowmode_seconds: 0,
                    default_auto_archive_minutes: Some(1440),
                    default_thread_slowmode_seconds: 0,
                    overwrites,
                },
            ],
        },
    };

    let files = export_channels(&source, guild_id(100), None).await.unwrap();
    assert!(files.definition_toml.contains("permissions_sync = true"));
    assert!(!files.definition_toml.contains("[channels.channel_300.overwrites"));

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

/// 初回 export でも Overwrite の Role/Member 対象へ決定的な論理 ID を割り当て、
/// 生成した参照宣言と state の対応を揃える。
#[tokio::test]
async fn initial_channel_export_registers_unmapped_overwrite_targets() {
    let permission = known_permission("VIEW_CHANNEL");
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![ChannelSnapshot {
                id: "300".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Text,
                manageable: true,
                name: "ルール".to_owned(),
                parent_id: None,
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
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
    assert!(rendered.contains("name: \"ルール\""));
    assert!(rendered.contains("parent: information"));
    assert!(!rendered.contains("TaggedLogicalId"));
    assert!(!rendered.contains("PhantomData"));
    assert!(rendered.contains("nsfw: false"));
    assert!(rendered.contains("slowmode_seconds: 5"));
    assert!(rendered.contains("default_auto_archive_minutes: 1440"));
    assert!(rendered.contains("default_thread_slowmode_seconds: 0"));
}

/// Channel の name は引用符と改行を含んでも plan 上で安全に表示する。
#[tokio::test]
async fn channel_plan_quotes_name_changes_with_json_escaping() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![channel_snapshot("300", ChannelKind::Text, "旧\n名", None)],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "新\"名\n改行"
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

    assert!(plan.render().contains("name: \"旧\\n名\" -> \"新\\\"名\\n改行\""));
}

/// Text の topic に空文字を指定した場合、解除ではなく空文字の設定として扱う。
#[tokio::test]
async fn channel_topic_empty_string_is_set_and_round_trips_without_a_plan() {
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
    });
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
        topic = ""
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
    let attributes = plan
        .get(&ChannelLogicalId::parse("rules").unwrap())
        .and_then(ChannelChange::attributes)
        .expect("空文字 topic の差分が plan に含まれます");
    assert_eq!(attributes.topic().unwrap().desired(), Some(&String::new()));

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
    assert_eq!(result.status, ChannelApplyStatus::Complete);
    assert_eq!(source.catalog.lock().unwrap().channels[0].topic.as_deref(), Some(""));

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

/// Channel の変更計画は overwrite の対象を snowflake の安定した表記で表示する。
#[tokio::test]
async fn channel_plan_renders_overwrite_targets_readably() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
        },
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        mode = "reference"
        [members.alice]
        mode = "reference"
        [channels.rules]
        type = "text"
        name = "ルール"
        [channels.rules.overwrites.everyone]
        VIEW_CHANNEL = "deny"
        [channels.rules.overwrites."role:moderator"]
        VIEW_CHANNEL = "allow"
        [channels.rules.overwrites."member:alice"]
        VIEW_CHANNEL = "deny"
    "#;
    let state = r#"
        {
            "schema_version": 1,
            "guild_id": "100",
            "roles": {"moderator": "400"},
            "members": {"alice": "500"},
            "channels": {"rules": "300"}
        }
    "#;
    let plan = plan_channels(&source, &test_permission_vocabulary(), guild_id(100), definition, state)
        .await
        .unwrap();

    let rendered = plan.render();
    assert!(rendered.contains("overwrites.everyone.VIEW_CHANNEL"));
    assert!(rendered.contains("overwrites.role:400.VIEW_CHANNEL"));
    assert!(rendered.contains("overwrites.member:500.VIEW_CHANNEL"));
    assert!(!rendered.contains("ChannelOverwriteTarget"));
    assert!(!rendered.contains("PhantomData"));
}

/// Discord が Thread の既定 slowmode を省略して返しても、0 として扱い再計画しない。
#[tokio::test]
async fn channel_default_thread_slowmode_zero_is_canonical_after_apply() {
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![ChannelSnapshot {
            id: "300".parse().unwrap(),
            position: 0,
            kind: ChannelKind::Text,
            manageable: true,
            name: "ルール".to_owned(),
            parent_id: None,
            topic: None,
            nsfw: false,
            slowmode_seconds: 0,
            default_auto_archive_minutes: Some(1440),
            default_thread_slowmode_seconds: 0,
            overwrites: BTreeMap::new(),
        }],
    });
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        name = "ルール"
        default_thread_slowmode_seconds = { default = true }
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
    assert!(plan.is_empty(), "canonical な 0 に対して不要な更新を計画しません");
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
    assert!(rendered.contains("default_thread_slowmode_seconds: 0"));
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
        [order.children]
        information = ["rules"]
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
            position: 0,
            kind: ChannelKind::Text,
            manageable: true,
            name: "旧ルール".to_owned(),
            parent_id: Some("200".parse().unwrap()),
            topic: Some("古い案内".to_owned()),
            nsfw: false,
            slowmode_seconds: 0,
            default_auto_archive_minutes: Some(1440),
            default_thread_slowmode_seconds: 10,
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
        default_thread_slowmode_seconds = { clear = true }
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
        assert_eq!(channel.default_thread_slowmode_seconds, 0);
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
            position: 0,
            kind: ChannelKind::Text,
            manageable: true,
            name: "ルール".to_owned(),
            parent_id: None,
            topic: None,
            nsfw: false,
            slowmode_seconds: 0,
            default_auto_archive_minutes: Some(1440),
            default_thread_slowmode_seconds: 0,
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
            position: 0,
            kind: ChannelKind::Text,
            manageable: true,
            name: "ルール".to_owned(),
            parent_id: None,
            topic: None,
            nsfw: false,
            slowmode_seconds: 0,
            default_auto_archive_minutes: Some(1440),
            default_thread_slowmode_seconds: 0,
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
                position: 0,
                kind: ChannelKind::Text,
                manageable: true,
                name: "ルール".to_owned(),
                parent_id: Some("200".parse().unwrap()),
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
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

/// 明示した Category 同期は、子 Channel の権限を Category の完成形へ揃える計画を作る。
#[tokio::test]
async fn channel_plan_reports_category_permission_sync() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![
                ChannelSnapshot {
                    id: "200".parse().unwrap(),
                    position: 0,
                    kind: ChannelKind::Category,
                    manageable: true,
                    name: "案内".to_owned(),
                    parent_id: None,
                    topic: None,
                    nsfw: false,
                    slowmode_seconds: 0,
                    default_auto_archive_minutes: Some(1440),
                    default_thread_slowmode_seconds: 0,
                    overwrites: BTreeMap::from([(
                        ChannelOverwriteTarget::Everyone,
                        ChannelOverwritePermissions::from_known(BTreeMap::from([(
                            view_channel.clone(),
                            OverwriteValue::Allow,
                        )])),
                    )]),
                },
                ChannelSnapshot {
                    id: "300".parse().unwrap(),
                    position: 0,
                    kind: ChannelKind::Text,
                    manageable: true,
                    name: "ルール".to_owned(),
                    parent_id: Some("200".parse().unwrap()),
                    topic: None,
                    nsfw: false,
                    slowmode_seconds: 0,
                    default_auto_archive_minutes: Some(1440),
                    default_thread_slowmode_seconds: 0,
                    overwrites: BTreeMap::from([(
                        ChannelOverwriteTarget::Everyone,
                        ChannelOverwritePermissions::from_known(BTreeMap::from([(view_channel, OverwriteValue::Deny)])),
                    )]),
                },
            ],
        },
    };
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        [channels.information.overwrites.everyone]
        VIEW_CHANNEL = "allow"
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = "information"
        permissions_sync = true
    "#;
    let state = channel_state(r#"{"information":"200","rules":"300"}"#);

    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state,
    )
    .await
    .expect("Category 同期の定義を plan できます");

    assert!(
        plan.get(&ChannelLogicalId::parse("rules").unwrap())
            .is_some_and(ChannelChange::is_update)
    );
}

/// Category 同期の apply は、個別権限の差分ではなく Category の全量 overwrite を送る。
#[tokio::test]
async fn channel_apply_copies_category_permission_overwrites() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let category_overwrites = BTreeMap::from([(
        ChannelOverwriteTarget::Everyone,
        ChannelOverwritePermissions::from_known(BTreeMap::from([(view_channel.clone(), OverwriteValue::Allow)])),
    )]);
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![
            ChannelSnapshot {
                id: "200".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Category,
                manageable: true,
                name: "案内".to_owned(),
                parent_id: None,
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
                overwrites: category_overwrites.clone(),
            },
            ChannelSnapshot {
                id: "300".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Text,
                manageable: true,
                name: "ルール".to_owned(),
                parent_id: Some("200".parse().unwrap()),
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
                overwrites: BTreeMap::new(),
            },
        ],
    });
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "案内"
        [channels.information.overwrites.everyone]
        VIEW_CHANNEL = "allow"
        [channels.rules]
        type = "text"
        name = "ルール"
        parent = "information"
        permissions_sync = true
    "#;
    let state = channel_state(r#"{"information":"200","rules":"300"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state,
    )
    .await
    .unwrap();

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let updates = source.updates.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].overwrites, Some(category_overwrites));
}

/// Category 同期と子 Channel 固有の Overwrite を同時に指定した定義は、競合として拒否する。
#[tokio::test]
async fn channel_plan_rejects_category_permission_sync_conflicts() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![channel_snapshot("200", ChannelKind::Category, "案内", None)],
        },
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
        permissions_sync = true
        [channels.rules.overwrites.everyone]
        VIEW_CHANNEL = "deny"
    "#;

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state(r#"{"information":"200"}"#),
    )
    .await
    .expect_err("Category 同期と個別 Overwrite の併用は競合です");

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("permissions_sync") && message.contains("Overwrite"))
    );
}

/// Channel overwrite の Role/Member 対応は、定義と state だけでなく実 Guild の所属も plan 前に検証する。
#[tokio::test]
async fn channel_plan_validates_permission_reference_members_before_building() {
    let source = RejectingChannelReferenceSource {
        catalog: ChannelCatalog {
            channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
        },
    };
    let definition = r#"
        schema_version = 1
        [members.alice]
        mode = "reference"
        [channels.rules]
        type = "text"
        name = "ルール"
        [channels.rules.overwrites."member:alice"]
        VIEW_CHANNEL = "deny"
    "#;
    let state = r#"
        {
            "schema_version": 1,
            "guild_id": "100",
            "members": {"alice": "500"},
            "channels": {"rules": "300"}
        }
    "#;

    let error = plan_channels(&source, &test_permission_vocabulary(), guild_id(100), definition, state)
        .await
        .expect_err("Member overwrite の実在検証に失敗した plan は作成しません");

    assert!(matches!(
        error,
        ManagementError::InvalidState(message) if message.contains("Member") && message.contains("所属")
    ));
}

/// 確認済み plan の apply でも、実行直前に Channel overwrite の参照を再検証する。
#[tokio::test]
async fn channel_apply_revalidates_permission_reference_members_before_updating() {
    let catalog = ChannelCatalog {
        channels: vec![channel_snapshot("300", ChannelKind::Text, "ルール", None)],
    };
    let definition = r#"
        schema_version = 1
        [members.alice]
        mode = "reference"
        [channels.rules]
        type = "text"
        name = "ルール"
        [channels.rules.overwrites."member:alice"]
        VIEW_CHANNEL = "deny"
    "#;
    let state = r#"
        {
            "schema_version": 1,
            "guild_id": "100",
            "members": {"alice": "500"},
            "channels": {"rules": "300"}
        }
    "#;
    let plan = plan_channels(
        &ChannelCatalogSource {
            catalog: catalog.clone(),
        },
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        state,
    )
    .await
    .unwrap();
    let source = RejectingChannelReferenceSource { catalog };
    let lock = GuildApplyLock::default();
    let result = ChannelApplyWorkflow::new(&lock, &source, &test_permission_vocabulary())
        .apply_channel_updates(
            guild_id(100),
            definition,
            state,
            &plan,
            Instant::now() + Duration::from_secs(60),
        )
        .await
        .unwrap();

    assert!(matches!(
        result.status,
        ChannelApplyStatus::Failed(message) if message.contains("Member") && message.contains("所属")
    ));
}

/// 新規 Category とその子を同時に作る場合も、子の作成 payload は Category の overwrite を引き継ぐ。
#[tokio::test]
async fn channel_apply_syncs_new_child_with_new_category() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let source = lifecycle_channel_source(ChannelCatalog { channels: Vec::new() });
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "非公開"
        [channels.information.overwrites.everyone]
        VIEW_CHANNEL = "deny"
        [channels.rules]
        type = "text"
        name = "運営"
        parent = "information"
        permissions_sync = true
    "#;
    let state = channel_state("{}");
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state,
    )
    .await
    .expect("新規 Category と同期する子の plan を作れます");

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let creates = source.creates.lock().unwrap();
    assert_eq!(creates.len(), 2);
    assert_eq!(creates[1].overwrites.len(), 1);
    assert_eq!(
        creates[1]
            .overwrites
            .get(&ChannelOverwriteTarget::Everyone)
            .and_then(|permissions| permissions.known.get(&view_channel)),
        Some(&OverwriteValue::Deny)
    );
}

/// Category の権限変更と同期子の更新は、同じ plan 内で Category を先に適用する。
#[tokio::test]
async fn channel_apply_updates_category_before_syncing_child() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![
            ChannelSnapshot {
                id: "200".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Category,
                manageable: true,
                name: "非公開".to_owned(),
                parent_id: None,
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
                overwrites: BTreeMap::from([(
                    ChannelOverwriteTarget::Everyone,
                    ChannelOverwritePermissions::from_known(BTreeMap::from([(
                        view_channel.clone(),
                        OverwriteValue::Allow,
                    )])),
                )]),
            },
            ChannelSnapshot {
                id: "300".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Text,
                manageable: true,
                name: "運営".to_owned(),
                parent_id: Some("200".parse().unwrap()),
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
                overwrites: BTreeMap::from([(
                    ChannelOverwriteTarget::Everyone,
                    ChannelOverwritePermissions::from_known(BTreeMap::from([(
                        view_channel.clone(),
                        OverwriteValue::Allow,
                    )])),
                )]),
            },
        ],
    });
    let definition = r#"
        schema_version = 1
        [channels.information]
        type = "category"
        name = "非公開"
        [channels.information.overwrites.everyone]
        VIEW_CHANNEL = "deny"
        [channels.rules]
        type = "text"
        name = "運営"
        parent = "information"
        permissions_sync = true
    "#;
    let state = channel_state(r#"{"information":"200","rules":"300"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state,
    )
    .await
    .unwrap();
    assert!(
        plan.get(&ChannelLogicalId::parse("information").unwrap())
            .is_some_and(ChannelChange::is_update)
    );
    assert!(
        plan.get(&ChannelLogicalId::parse("rules").unwrap())
            .is_some_and(ChannelChange::is_update)
    );

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let updates = source.updates.lock().unwrap();
    assert_eq!(updates.len(), 2);
    let category_permissions = updates[0]
        .overwrites
        .as_ref()
        .and_then(|overwrites| overwrites.get(&ChannelOverwriteTarget::Everyone))
        .and_then(|permissions| permissions.known.get(&view_channel));
    let child_permissions = updates[1]
        .overwrites
        .as_ref()
        .and_then(|overwrites| overwrites.get(&ChannelOverwriteTarget::Everyone))
        .and_then(|permissions| permissions.known.get(&view_channel));
    assert_eq!(category_permissions, Some(&OverwriteValue::Deny));
    assert_eq!(child_permissions, Some(&OverwriteValue::Deny));
}

/// 論理 ID の辞書順に依存せず、Category の更新を同期対象の子より先に適用する。
#[tokio::test]
async fn channel_apply_orders_category_updates_before_child_updates() {
    let view_channel = known_permission("VIEW_CHANNEL");
    let source = lifecycle_channel_source(ChannelCatalog {
        channels: vec![
            ChannelSnapshot {
                id: "200".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Category,
                manageable: true,
                name: "旧カテゴリ".to_owned(),
                parent_id: None,
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
                overwrites: BTreeMap::from([(
                    ChannelOverwriteTarget::Everyone,
                    ChannelOverwritePermissions::from_known(BTreeMap::from([(
                        view_channel.clone(),
                        OverwriteValue::Allow,
                    )])),
                )]),
            },
            ChannelSnapshot {
                id: "300".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Text,
                manageable: true,
                name: "子".to_owned(),
                parent_id: Some("200".parse().unwrap()),
                topic: None,
                nsfw: false,
                slowmode_seconds: 0,
                default_auto_archive_minutes: Some(1440),
                default_thread_slowmode_seconds: 0,
                overwrites: BTreeMap::from([(
                    ChannelOverwriteTarget::Everyone,
                    ChannelOverwritePermissions::from_known(BTreeMap::from([(
                        view_channel.clone(),
                        OverwriteValue::Allow,
                    )])),
                )]),
            },
        ],
    });
    let definition = r#"
        schema_version = 1
        [channels.a_child]
        type = "text"
        name = "子"
        parent = "z_category"
        permissions_sync = true
        [channels.z_category]
        type = "category"
        name = "新カテゴリ"
        [channels.z_category.overwrites.everyone]
        VIEW_CHANNEL = "deny"
    "#;
    let state = channel_state(r#"{"z_category":"200","a_child":"300"}"#);
    let plan = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &state,
    )
    .await
    .unwrap();

    let result = apply_channels(
        &source,
        guild_id(100),
        definition,
        &state,
        &plan,
        Instant::now() + Duration::from_secs(60),
    )
    .await
    .unwrap();

    assert_eq!(result.status, ChannelApplyStatus::Complete);
    let updates = source.updates.lock().unwrap();
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].name.as_deref(), Some("新カテゴリ"));
    assert!(updates[1].name.is_none());
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

/// permission overwrite を持つ新規 Channel も MANAGE_ROLES がない状態で plan を拒否する。
#[tokio::test]
async fn channel_plan_rejects_overwrite_creates_without_manage_roles() {
    let source = PermissionAwareChannelSource {
        catalog: ChannelCatalog { channels: Vec::new() },
        can_manage_roles: false,
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        mode = "reference"
        [channels.rules]
        type = "text"
        name = "rules"
        [channels.rules.overwrites."role:moderator"]
        VIEW_CHANNEL = "allow"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {"moderator": "400"}
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

/// Announcement Channel の全対応属性を export し、再投入した plan が無差分になる。
#[tokio::test]
async fn announcement_channel_export_round_trip_is_idempotent() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog {
            channels: vec![ChannelSnapshot {
                id: "300".parse().unwrap(),
                position: 0,
                kind: ChannelKind::Announcement,
                manageable: true,
                name: "news".to_owned(),
                parent_id: None,
                topic: Some("updates".to_owned()),
                nsfw: false,
                slowmode_seconds: 5,
                default_auto_archive_minutes: Some(4320),
                default_thread_slowmode_seconds: 0,
                overwrites: BTreeMap::new(),
            }],
        },
    };

    let files = export_channels(&source, guild_id(100), None).await.unwrap();
    assert!(files.definition_toml.contains("type = \"announcement\""));
    assert!(files.definition_toml.contains("default_auto_archive_minutes = 4320"));
    assert!(!files.definition_toml.contains("default_thread_slowmode_seconds"));

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

/// Announcement Channel では Text 専用の Thread 低速モードを受理しない。
#[tokio::test]
async fn announcement_channel_rejects_default_thread_slowmode() {
    let source = ChannelCatalogSource {
        catalog: ChannelCatalog { channels: Vec::new() },
    };
    let definition = r#"
        schema_version = 1
        [channels.news]
        type = "announcement"
        name = "news"
        default_thread_slowmode_seconds = 5
    "#;

    let error = plan_channels(
        &source,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state("{}"),
    )
    .await
    .expect_err("Announcement の非対応属性を拒否します");
    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Announcement") && message.contains("default_thread_slowmode_seconds"))
    );
}

/// COMMUNITY feature がない Guild では、API 呼び出し前の plan で作成を拒否する。
#[tokio::test]
async fn announcement_channel_creation_requires_community_feature() {
    let definition = r#"
        schema_version = 1
        [channels.news]
        type = "announcement"
        name = "news"
    "#;

    let error = plan_channels(
        &FeaturelessChannelSource,
        &test_permission_vocabulary(),
        guild_id(100),
        definition,
        &channel_state("{}"),
    )
    .await
    .expect_err("COMMUNITY feature がない Guild では Announcement を作成できません");
    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("COMMUNITY")));
}
