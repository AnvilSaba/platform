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
            grantable_permissions: permission_names.clone(),
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

#[derive(Clone)]
struct BindFakeResourceSource {
    resource: ResourceLookup,
}

impl ResourceSource for BindFakeResourceSource {
    async fn lookup_resource(
        &self,
        _guild_id: &GuildId,
        _discord_id: u64,
    ) -> Result<Option<ResourceLookup>, ManagementError> {
        Ok(Some(self.resource))
    }
}

impl RoleSource for BindFakeResourceSource {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        Ok(RoleCatalog {
            roles: Vec::new(),
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::new(),
        })
    }
}

struct MissingResourceSource;

impl ResourceSource for MissingResourceSource {
    async fn lookup_resource(
        &self,
        _guild_id: &GuildId,
        _discord_id: u64,
    ) -> Result<Option<ResourceLookup>, ManagementError> {
        Ok(None)
    }
}

/// 明示した Role 宣言と Discord ID の bind が返却 state に保存されることを保証する。
#[tokio::test]
async fn bind_adopts_an_existing_role_into_state() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let result = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "moderator",
            "200",
        )
        .await
        .unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap(),
        serde_json::json!({
            "schema_version": 1,
            "guild_id": "100",
            "roles": { "moderator": "200" }
        })
    );
}

/// 明示した Channel 宣言を Role と分離した state 対応表へ保存できることを保証する。
#[tokio::test]
async fn bind_adopts_an_existing_channel_into_state() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Channel,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[channels.rules]\ntype = \"text\"\nname = \"ルール\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let result = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Channel,
            "rules",
            "300",
        )
        .await
        .unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap()["channels"]["rules"],
        "300"
    );
}

/// Guild 所属の Member 参照を宣言と対応付け、Member 用 state へ保存できることを保証する。
#[tokio::test]
async fn bind_adopts_an_existing_member_into_state() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Member,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[members.owner]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let result = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Member,
            "owner",
            "400",
        )
        .await
        .unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.state_json).unwrap()["members"]["owner"],
        "400"
    );
}

/// 指定した Discord ID の実体と要求した Role/Channel/Member の型が違えば bind しないことを保証する。
#[tokio::test]
async fn bind_rejects_a_resource_type_mismatch() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Channel,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "moderator",
            "300",
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ManagementError::ResourceTypeMismatch {
            expected: ResourceType::Role,
            actual: ResourceType::Channel,
            discord_id: 300,
        }
    );
}

/// 実体が別 Guild に属している場合は、対象 Guild の state へ bind しないことを保証する。
#[tokio::test]
async fn bind_rejects_a_resource_from_another_guild() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(999),
        },
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "moderator",
            "200",
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ManagementError::ResourceGuildMismatch {
            resource_type: ResourceType::Role,
            discord_id: 200,
            resource_guild_id: guild_id(999),
            actual_guild_id: guild_id(100),
        }
    );
}

/// 既存 state が採用済みの Discord ID を別の論理 ID へ重複 bind できないことを保証する。
#[tokio::test]
async fn bind_rejects_duplicate_resource_adoption() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{"existing":"200"}}"#;

    let error = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "moderator",
            "200",
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("existing") && message.contains("200")));
}

/// 既存の論理 ID に別の Discord ID を指定した暗黙の付け替えを拒否することを保証する。
#[tokio::test]
async fn bind_rejects_retargeting_an_existing_logical_id() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{"moderator":"201"}}"#;

    let error = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "moderator",
            "200",
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("moderator") && message.contains("201") && message.contains("200")));
}

/// definition にない論理 ID は、Discord への照会前に bind を拒否することを保証する。
#[tokio::test]
async fn bind_rejects_an_undeclared_logical_id() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "missing",
            "200",
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("missing") && message.contains("宣言")));
}

/// 参照専用 Channel の宣言があっても対応がなければ plan を開始しないことを保証する。
#[tokio::test]
async fn plan_rejects_a_reference_channel_without_a_binding() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = "schema_version = 1\n[channels.information]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = service
        .plan_roles(guild_id(100), definition, state)
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("Channel information") && message.contains("対応がありません")));
}

/// Role plan が未実装の Channel 属性管理を黙って無視しないことを保証する。
#[tokio::test]
async fn plan_rejects_managed_channel_attributes() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = "schema_version = 1\n[channels.information]\ntype = \"text\"\nname = \"案内\"\n";
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "channels": {"information": "300"}
    }"#;

    let error = service.plan_roles(guild_id(100), definition, state).await.unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Channel information") && message.contains("対象外")));
}

/// Role・Channel・Member の参照専用宣言は対応だけを解決し、実属性を変更差分にしないことを保証する。
#[tokio::test]
async fn plan_resolves_bound_reference_resources_without_managing_attributes() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "外部管理")],
    });
    let definition = r#"
        schema_version = 1
        [roles.external]
        mode = "reference"
        [channels.information]
        mode = "reference"
        [members.owner]
        mode = "reference"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {"external": "200"},
        "channels": {"information": "300"},
        "members": {"owner": "400"}
    }"#;

    let plan = service.plan_roles(guild_id(100), definition, state).await.unwrap();

    assert!(plan.changes.is_empty());
}

/// Channel の親として使う論理 ID の宣言が不足していれば plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_a_channel_reference_without_a_declaration() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        parent = "information"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "channels": {"rules": "300"}
    }"#;

    let error = service
        .plan_roles(guild_id(100), definition, state)
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("information") && message.contains("宣言がありません")));
}

/// 管理メッセージ群の投稿先 Channel が未宣言なら plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_a_message_set_channel_reference_without_a_declaration() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = r#"
        schema_version = 1
        [message_sets.guidelines]
        channel = "information"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {}
    }"#;

    let error = service.plan_roles(guild_id(100), definition, state).await.unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("information") && message.contains("宣言がありません")));
}

/// 独立した管理スレッドの投稿先 Channel に対応がなければ plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_a_thread_channel_reference_without_a_binding() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = r#"
        schema_version = 1
        [channels.information]
        mode = "reference"
        [threads.details]
        channel = "information"
        name = "詳細"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {}
    }"#;

    let error = service.plan_roles(guild_id(100), definition, state).await.unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("information") && message.contains("対応がありません")));
}

/// 予約論理 ID everyone は state へ bind せず、Guild ID へ解決する契約を保証する。
#[tokio::test]
async fn bind_rejects_the_reserved_everyone_role() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[roles.everyone]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "everyone",
            "100",
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("everyone") && message.contains("bind")));
}

/// @everyone の実 ID を別の論理 ID に bind できないことを保証する。
#[tokio::test]
async fn bind_rejects_an_alias_for_reserved_everyone_role() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[roles.default_role]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = service
        .bind_resource(
            guild_id(100),
            definition,
            state,
            ResourceType::Role,
            "default_role",
            "100",
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("everyone") && message.contains("別の論理 ID")));
}

/// Channel の state 対応表で一つの実体を複数の論理 ID が採用する状態を拒否することを保証する。
#[tokio::test]
async fn plan_rejects_duplicate_channel_snowflakes() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = "schema_version = 1\n";
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "channels": {"first": "300", "second": "300"}
    }"#;

    let error = service
        .plan_roles(guild_id(100), definition, state)
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("Channel") && message.contains("同じ Snowflake 300")));
}

/// Member の state 対応表で一つの実体を複数の論理 ID が採用する状態を拒否することを保証する。
#[tokio::test]
async fn plan_rejects_duplicate_member_snowflakes() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = "schema_version = 1\n";
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "members": {"first": "400", "second": "400"}
    }"#;

    let error = service
        .plan_roles(guild_id(100), definition, state)
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("Member") && message.contains("同じ Snowflake 400")));
}

/// bind の返却 state をそのまま次の plan へ渡せば参照専用 Channel を解決できることを保証する。
#[tokio::test]
async fn bound_state_can_be_passed_to_the_next_plan() {
    let service = RoleManagementService::new(BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Channel,
            guild_id: guild_id(100),
        },
    });
    let definition = "schema_version = 1\n[channels.information]\nmode = \"reference\"\n";
    let initial_state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;
    let bound = service
        .bind_resource(
            guild_id(100),
            definition,
            initial_state,
            ResourceType::Channel,
            "information",
            "300",
        )
        .await
        .unwrap();

    let plan = service
        .plan_roles(guild_id(100), definition, &bound.state_json)
        .await
        .unwrap();

    assert!(plan.changes.is_empty());
}

/// Role・Channel・Member・管理メッセージを含む定義サンプルを resource 定義として読み取れることを保証する。
#[test]
fn management_sample_accepts_resource_declarations() {
    let sample = include_str!("../../../../../../docs/examples/discord-management.base.toml");
    toml::from_str::<DefinitionFile>(sample).unwrap();
}

/// 同名Roleが複数あっても、名前ではなくSnowflake由来の論理IDで一意にexportできることを保証する。
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

/// @everyoneを予約論理IDでexportし、stateから除外しつつ、権限のtrue/falseをすべて保持できることを保証する。
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

/// @everyoneへ名前など権限以外の管理属性を指定した定義を拒否し、Discord固有の制約を守る。
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

/// 再export時に既存stateの論理IDを引き継ぎ、Role名の変更で対応関係が変わらないことを保証する。
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

/// 同じGuild状態から生成したdefinitionとstateをそのままplanすると差分が生じないことを保証する。
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

/// 参照専用Roleは更新しないため、Botが管理不能なRoleでも参照先として使用できることを保証する。
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

/// stateに対応を持たない@everyoneも、予約論理IDによって参照専用Roleとして解決できることを保証する。
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

/// 権限のdefault指定が固定値ではなく、Guildの@everyoneに設定された基底権限を参照することを保証する。
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

/// export対象をBotが管理可能なRoleに限定し、管理不能なRoleを編集用定義へ混入させない。
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

/// Role設定セットで指定した属性が合成され、実際のRoleとの差分としてplanされることを保証する。
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

/// Guild 内に対象が存在しなければ、対応 state を変更せずに bind を拒否することを保証する。
#[tokio::test]
async fn bind_rejects_a_missing_resource() {
    let service = RoleManagementService::new(MissingResourceSource);
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = service
        .bind_resource(guild_id(100), definition, state, ResourceType::Role, "moderator", "200")
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ManagementError::ResourceNotFound {
            resource_type: ResourceType::Role,
            discord_id: 200,
        }
    );
}

/// Channel の Role overwrite が未宣言の Role を参照していれば plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_an_undeclared_role_overwrite_target() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        [channels.rules.overwrites."role:admin"]
        VIEW_CHANNEL = "allow"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "channels": {"rules": "300"}
    }"#;

    let error = service.plan_roles(guild_id(100), definition, state).await.unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Role admin") && message.contains("宣言がありません")));
}

/// Channel の Member overwrite が未宣言の Member を参照していれば plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_an_undeclared_member_overwrite_target() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    });
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        [channels.rules.overwrites."member:moderator"]
        VIEW_CHANNEL = "allow"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "channels": {"rules": "300"}
    }"#;

    let error = service.plan_roles(guild_id(100), definition, state).await.unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Member moderator") && message.contains("宣言がありません")));
}

/// 複数設定セットの後勝ちと直接指定の最優先を確認し、省略属性を変更しない合成規則を保証する。
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

/// 各属性のdefault指定がschemaで定めた既定値へ解決され、必要な差分だけがplanされることを保証する。
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

/// stateのGuildが要求先と異なる場合、Discordへ問い合わせる前に誤投入として拒否することを保証する。
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

/// Snowflake形式でないGuild IDをデシリアライズ時に拒否し、不正なstateを後段へ渡さない。
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

/// 予約論理ID everyoneをstateに保存する旧・不正形式を、stateモデルの検証で拒否することを保証する。
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

/// @everyone の実 ID を別の論理 ID に保存した state も拒否することを保証する。
#[tokio::test]
async fn everyone_role_id_under_another_logical_id_is_rejected() {
    let service = RoleManagementService::new(StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    });
    let error = service
        .plan_roles(
            guild_id(100),
            "schema_version = 1\n[roles.default_role]\nmode = \"reference\"",
            &state("100", r#"{"default_role":"100"}"#),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("everyone") && message.contains("別の論理 ID")));
}

/// 複数の論理IDが同じRole Snowflakeを指す曖昧なstateを拒否することを保証する。
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

/// JSONの重複キーがMap変換で黙って上書きされず、不正なstateとして報告されることを保証する。
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

/// 新規Role用に生成する論理IDが古いstateの予約と衝突した場合、誤対応せず拒否することを保証する。
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

/// タイプミスなどの未知フィールドを無視せず、definitionの入力エラーとして報告することを保証する。
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

/// Roleから未参照の設定セットも検証し、潜在的な不正定義を見逃さないことを保証する。
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

/// 未参照設定セット内でも無効なdefault指定を拒否し、定義全体を一貫して検証する。
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

/// 未参照設定セット内の未知権限も検出し、将来参照された際の遅延エラーを防ぐ。
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

/// 対応外schema_versionのdefinitionを拒否し、異なる解釈でRoleを更新しないことを保証する。
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

/// 利用者向けサンプルTOMLが現在のDefinitionFileとして読み取れる状態を維持する。
#[test]
fn editor_sample_matches_the_supported_definition_shape() {
    let sample = include_str!("../../../../../../docs/examples/discord-role-management.toml");
    toml::from_str::<DefinitionFile>(sample).unwrap();
}

/// Roleが存在しない設定セットを参照した場合、合成処理へ進む前に定義エラーとして報告する。
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

/// 参照専用Roleへの管理属性指定を拒否し、更新されない設定を利用者に誤認させない。
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

/// Botが持たない権限のfalseからtrueへの変更をplanで拒否し、Discord APIの失敗を事前に示す。
#[tokio::test]
async fn plan_rejects_granting_a_permission_the_bot_does_not_have() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::from(["SEND_MESSAGES".to_owned(), "VIEW_CHANNEL".to_owned()]),
            grantable_permissions: BTreeSet::from(["VIEW_CHANNEL".to_owned()]),
            default_permissions: BTreeMap::from([
                ("SEND_MESSAGES".to_owned(), false),
                ("VIEW_CHANNEL".to_owned(), true),
            ]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let service = RoleManagementService::new(source);
    let definition = "schema_version = 1\n[roles.moderator.permissions]\nSEND_MESSAGES = true\n";

    let error = service
        .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("SEND_MESSAGES") && message.contains("Bot 自身")));
}

/// Botが現在持たない権限でも、対象Roleから削除する変更は付与ではないためplanできることを保証する。
#[tokio::test]
async fn plan_allows_removing_a_permission_the_bot_does_not_have() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "運営")],
            permission_names: BTreeSet::from(["SEND_MESSAGES".to_owned(), "VIEW_CHANNEL".to_owned()]),
            grantable_permissions: BTreeSet::new(),
            default_permissions: BTreeMap::from([
                ("SEND_MESSAGES".to_owned(), false),
                ("VIEW_CHANNEL".to_owned(), true),
            ]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let service = RoleManagementService::new(source);
    let definition = "schema_version = 1\n[roles.moderator.permissions]\nVIEW_CHANNEL = false\n";

    let plan = service
        .plan_roles(guild_id(100), definition, &state("100", r#"{"moderator":"200"}"#))
        .await
        .unwrap();

    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].attribute, "permissions.VIEW_CHANNEL");
}

/// stateに対応を持たないeveryoneもGuild IDへ解決され、planどおりにapplyされることを保証する。
#[tokio::test]
async fn apply_resolves_everyone_without_a_state_mapping() {
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("100", "@everyone")],
            permission_names: BTreeSet::from(["SEND_MESSAGES".to_owned(), "VIEW_CHANNEL".to_owned()]),
            grantable_permissions: BTreeSet::from(["SEND_MESSAGES".to_owned(), "VIEW_CHANNEL".to_owned()]),
            default_permissions: BTreeMap::from([
                ("SEND_MESSAGES".to_owned(), false),
                ("VIEW_CHANNEL".to_owned(), true),
            ]),
        })),
        updates: Arc::new(Mutex::new(Vec::new())),
        outcome: RoleUpdateOutcome::Applied,
        apply_update: true,
    };
    let service = RoleManagementService::new(source.clone());
    let definition = "schema_version = 1\n[roles.everyone.permissions]\nSEND_MESSAGES = true\n";
    let state = state("100", "{}");
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

    assert_eq!(result.status, RoleApplyStatus::Complete);
    assert_eq!(result.applied, plan.changes);
    assert!(result.pending.is_empty());
    assert_eq!(source.updates.lock().unwrap().len(), 1);
}

/// applyが明示属性だけを更新し、省略された権限や属性を現在値のまま保持することを保証する。
#[tokio::test]
async fn apply_updates_only_explicit_attributes_and_preserves_omitted_permissions() {
    let mut moderator = role("200", "運営");
    moderator.permissions =
        BTreeMap::from([("VIEW_CHANNEL".to_owned(), false), ("MANAGE_MESSAGES".to_owned(), true)]);
    let source = ApplyingFakeRoleSource {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![moderator],
            permission_names: BTreeSet::from(["VIEW_CHANNEL".to_owned(), "MANAGE_MESSAGES".to_owned()]),
            grantable_permissions: BTreeSet::from(["VIEW_CHANNEL".to_owned(), "MANAGE_MESSAGES".to_owned()]),
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

/// 更新応答が不明で再取得値も希望値と違う場合、成功扱いせず以降の更新を停止する。
#[tokio::test]
async fn unknown_update_response_stops_when_refetched_value_does_not_match() {
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

/// 更新期限超過時に進捗を不明として返し、未完了変更を安全に再投入できることを保証する。
#[tokio::test]
async fn update_deadline_returns_unknown_progress_that_can_be_resubmitted() {
    let catalog = RoleCatalog {
        roles: vec![role("200", "運営")],
        permission_names: BTreeSet::new(),
        grantable_permissions: BTreeSet::new(),
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

/// 途中の更新失敗で処理を停止し、適用済みと未適用の差分を正確に分けて返すことを保証する。
#[tokio::test]
async fn apply_stops_at_first_failure_and_reports_successful_and_pending_changes() {
    let source = FailsOnSecondUpdate {
        catalog: Arc::new(Mutex::new(RoleCatalog {
            roles: vec![role("200", "A"), role("201", "B")],
            permission_names: BTreeSet::new(),
            grantable_permissions: BTreeSet::new(),
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

/// 同一Guildへの並行applyを待機させず拒否し、競合更新と二重適用を防ぐ。
#[tokio::test]
async fn concurrent_apply_for_the_same_guild_is_rejected_without_waiting() {
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
