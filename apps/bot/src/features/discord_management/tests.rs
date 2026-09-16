use super::{
    GuildApplyLock,
    apply::{RoleApplyOptions, RoleApplyResult, RoleApplyStatus, RoleApplyWorkflow},
    bind::{BindResult, BindWorkflow},
    configuration::*,
    domain::*,
    export::{export_channels, export_roles},
    ids::*,
    plan::{plan_channels, plan_roles as plan_roles_workflow},
    port::*,
    resource::channel::{
        ChannelChange, ChannelPlan,
        apply::{ChannelApplyOptions, ChannelApplyResult, ChannelApplyStatus, ChannelApplyWorkflow},
    },
    resource::role::{Change, RolePlan},
};
use std::{
    collections::{BTreeMap, BTreeSet},
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
        color: Color::default(),
        hoist: false,
        mentionable: false,
        permissions: known_permission_values([("SEND_MESSAGES", false), ("VIEW_CHANNEL", true)]),
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

fn test_permission_vocabulary() -> PermissionVocabulary {
    PermissionVocabulary::from_names(["MANAGE_MESSAGES", "MANAGE_ROLES", "SEND_MESSAGES", "VIEW_CHANNEL"])
        .expect("テスト用の権限語彙は字句的に妥当です")
}

fn known_permission(name: &str) -> KnownPermission {
    let vocabulary = test_permission_vocabulary();
    let name = PermissionName::parse(name).expect("テスト用の権限名は字句的に妥当です");
    vocabulary.resolve(&name).expect("テスト用の権限名は語彙に含まれます")
}

fn known_permissions<I, S>(names: I) -> BTreeSet<KnownPermission>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    names.into_iter().map(|name| known_permission(name.as_ref())).collect()
}

fn known_permission_values<I, S>(values: I) -> BTreeMap<KnownPermission, bool>
where
    I: IntoIterator<Item = (S, bool)>,
    S: AsRef<str>,
{
    values
        .into_iter()
        .map(|(name, value)| (known_permission(name.as_ref()), value))
        .collect()
}

fn parse_definition(contents: &str) -> Result<DefinitionFile, ManagementError> {
    DefinitionFile::parse(contents, &test_permission_vocabulary())
}

async fn plan_roles<S: RoleSource>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
) -> Result<RolePlan, ManagementError> {
    let vocabulary = test_permission_vocabulary();
    plan_roles_workflow(source, &vocabulary, guild_id, definition_toml, state_json).await
}

async fn bind_resource<S: ResourceSource>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    resource_type: ResourceType,
    logical_id: &str,
    discord_id: &str,
) -> Result<BindResult, ManagementError> {
    let vocabulary = test_permission_vocabulary();
    BindWorkflow::new(source, &vocabulary)
        .bind_resource(
            guild_id,
            definition_toml,
            state_json,
            resource_type,
            logical_id,
            discord_id,
        )
        .await
}

async fn apply_role_updates<S: RoleUpdater>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    let apply_lock = GuildApplyLock::default();
    let vocabulary = test_permission_vocabulary();
    RoleApplyWorkflow::new(&apply_lock, source, &vocabulary)
        .apply_role_updates(
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            processing_deadline,
        )
        .await
}

async fn apply_roles<S: RoleLifecycleTarget>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    let apply_lock = GuildApplyLock::default();
    let vocabulary = test_permission_vocabulary();
    RoleApplyWorkflow::new(&apply_lock, source, &vocabulary)
        .apply_roles(
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            processing_deadline,
        )
        .await
}

async fn apply_roles_with_options<S: RoleLifecycleTarget>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &RolePlan,
    options: RoleApplyOptions,
    processing_deadline: Instant,
) -> Result<RoleApplyResult, ManagementError> {
    let apply_lock = GuildApplyLock::default();
    let vocabulary = test_permission_vocabulary();
    RoleApplyWorkflow::new(&apply_lock, source, &vocabulary)
        .apply_roles_with_options(
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            options,
            processing_deadline,
        )
        .await
}

async fn apply_channels<S: ChannelLifecycleTarget>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &ChannelPlan,
    processing_deadline: Instant,
) -> Result<ChannelApplyResult, ManagementError> {
    let apply_lock = GuildApplyLock::default();
    let vocabulary = test_permission_vocabulary();
    ChannelApplyWorkflow::new(&apply_lock, source, &vocabulary)
        .apply_channels(
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            processing_deadline,
        )
        .await
}

async fn apply_channels_with_options<S: ChannelLifecycleTarget>(
    source: &S,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    confirmed_plan: &ChannelPlan,
    options: ChannelApplyOptions,
    processing_deadline: Instant,
) -> Result<ChannelApplyResult, ManagementError> {
    let apply_lock = GuildApplyLock::default();
    let vocabulary = test_permission_vocabulary();
    ChannelApplyWorkflow::new(&apply_lock, source, &vocabulary)
        .apply_channels_with_options(
            guild_id,
            definition_toml,
            state_json,
            confirmed_plan,
            options,
            processing_deadline,
        )
        .await
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

struct CatalogMustNotBeRead;

impl RoleSource for CatalogMustNotBeRead {
    async fn role_catalog(&self, _guild_id: &GuildId) -> Result<RoleCatalog, ManagementError> {
        panic!("未知権限の解決に失敗した定義は Role catalog へ進めてはいけません");
    }
}

/// 明示した Role 宣言と Discord ID の bind が返却 state に保存されることを保証する。
#[tokio::test]
async fn bind_adopts_an_existing_role_into_state() {
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let result = bind_resource(
        &source,
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
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Channel,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[channels.rules]\ntype = \"text\"\nname = \"ルール\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let result = bind_resource(
        &source,
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
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Member,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[members.owner]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let result = bind_resource(
        &source,
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
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Channel,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = bind_resource(
        &source,
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
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(999),
        },
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = bind_resource(
        &source,
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
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{"existing":"200"}}"#;

    let error = bind_resource(
        &source,
        guild_id(100),
        definition,
        state,
        ResourceType::Role,
        "moderator",
        "200",
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("existing") && message.contains("200"))
    );
}

/// 既存の論理 ID に別の Discord ID を指定した暗黙の付け替えを拒否することを保証する。
#[tokio::test]
async fn bind_rejects_retargeting_an_existing_logical_id() {
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{"moderator":"201"}}"#;

    let error = bind_resource(
        &source,
        guild_id(100),
        definition,
        state,
        ResourceType::Role,
        "moderator",
        "200",
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("moderator") && message.contains("201") && message.contains("200"))
    );
}

/// definition にない論理 ID は、Discord への照会前に bind を拒否することを保証する。
#[tokio::test]
async fn bind_rejects_an_undeclared_logical_id() {
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = bind_resource(
        &source,
        guild_id(100),
        definition,
        state,
        ResourceType::Role,
        "missing",
        "200",
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("missing") && message.contains("宣言"))
    );
}

/// 参照専用 Channel の宣言があっても対応がなければ plan を開始しないことを保証する。
#[tokio::test]
async fn plan_rejects_a_reference_channel_without_a_binding() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let definition = "schema_version = 1\n[channels.information]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("Channel information") && message.contains("対応がありません"))
    );
}

/// Role・Channel・Member の参照専用宣言は対応だけを解決し、実属性を変更差分にしないことを保証する。
#[tokio::test]
async fn plan_resolves_bound_reference_resources_without_managing_attributes() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "外部管理")],
    };
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

    let plan = plan_roles(&source, guild_id(100), definition, state).await.unwrap();

    assert!(plan.is_empty());
}

/// Channel の親として使う論理 ID の宣言が不足していれば plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_a_channel_reference_without_a_declaration() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
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

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("information") && message.contains("宣言がありません"))
    );
}

/// 管理メッセージ群の投稿先 Channel が未宣言なら plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_a_message_set_channel_reference_without_a_declaration() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
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

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("information") && message.contains("宣言がありません"))
    );
}

/// 独立した管理スレッドの投稿先 Channel に対応がなければ plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_a_thread_channel_reference_without_a_binding() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
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

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("information") && message.contains("対応がありません"))
    );
}

/// 予約論理 ID everyone は state へ bind せず、Guild ID へ解決する契約を保証する。
#[tokio::test]
async fn bind_rejects_the_reserved_everyone_role() {
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[roles.everyone]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = bind_resource(
        &source,
        guild_id(100),
        definition,
        state,
        ResourceType::Role,
        "everyone",
        "100",
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("everyone") && message.contains("bind"))
    );
}

/// @everyone の実 ID を別の論理 ID に bind できないことを保証する。
#[tokio::test]
async fn bind_rejects_an_alias_for_reserved_everyone_role() {
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Role,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[roles.default_role]\nmode = \"reference\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = bind_resource(
        &source,
        guild_id(100),
        definition,
        state,
        ResourceType::Role,
        "default_role",
        "100",
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("everyone") && message.contains("別の論理 ID"))
    );
}

/// Channel の state 対応表で一つの実体を複数の論理 ID が採用する状態を拒否することを保証する。
#[tokio::test]
async fn plan_rejects_duplicate_channel_snowflakes() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let definition = "schema_version = 1\n";
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "channels": {"first": "300", "second": "300"}
    }"#;

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("Channel") && message.contains("同じ Snowflake 300"))
    );
}

/// Member の state 対応表で一つの実体を複数の論理 ID が採用する状態を拒否することを保証する。
#[tokio::test]
async fn plan_rejects_duplicate_member_snowflakes() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let definition = "schema_version = 1\n";
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {},
        "members": {"first": "400", "second": "400"}
    }"#;

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("Member") && message.contains("同じ Snowflake 400"))
    );
}

/// bind の返却 state をそのまま次の plan へ渡せば参照専用 Channel を解決できることを保証する。
#[tokio::test]
async fn bound_state_can_be_passed_to_the_next_plan() {
    let source = BindFakeResourceSource {
        resource: ResourceLookup {
            resource_type: ResourceType::Channel,
            guild_id: guild_id(100),
        },
    };
    let definition = "schema_version = 1\n[channels.information]\nmode = \"reference\"\n";
    let initial_state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;
    let bound = bind_resource(
        &source,
        guild_id(100),
        definition,
        initial_state,
        ResourceType::Channel,
        "information",
        "300",
    )
    .await
    .unwrap();

    let plan = plan_roles(&source, guild_id(100), definition, &bound.state_json)
        .await
        .unwrap();

    assert!(plan.is_empty());
}

/// Role・Channel・Member・管理メッセージを含む定義サンプルを resource 定義として読み取れることを保証する。
#[test]
fn management_sample_accepts_resource_declarations() {
    let sample = include_str!("../../../../../docs/examples/discord-management.base.toml");
    parse_definition(sample).unwrap();
}

/// 同名Roleが複数あっても、名前ではなくSnowflake由来の論理IDで一意にexportできることを保証する。
#[tokio::test]
async fn initial_export_uses_snowflakes_for_duplicate_role_names() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営"), role("201", "運営")],
    };

    let files = export_roles(&source, guild_id(100), None).await.unwrap();
    let definition = parse_definition(&files.definition_toml).unwrap();
    let state = StateFile::parse_for_guild(&files.state_json, guild_id(100)).unwrap();

    assert_eq!(
        literal_string(&definition.roles[&logical_id("role_200")].attributes().name),
        Some("運営")
    );
    assert_eq!(
        literal_string(&definition.roles[&logical_id("role_201")].attributes().name),
        Some("運営")
    );
    assert!(
        !definition.roles[&logical_id("role_200")]
            .attributes()
            .permissions
            .contains_key("SEND_MESSAGES")
    );
    assert_eq!(state.guild_id.get(), 100);
    assert_eq!(state.roles[&logical_id("role_200")].get(), 200);
    assert_eq!(state.roles[&logical_id("role_201")].get(), 201);
}

/// @everyoneを予約論理IDでexportし、stateから除外しつつ、権限のtrue/falseをすべて保持できることを保証する。
#[tokio::test]
async fn everyone_export_contains_only_permissions_and_keeps_false_values() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };

    let files = export_roles(&source, guild_id(100), None).await.unwrap();
    let definition = parse_definition(&files.definition_toml).unwrap();
    let state = StateFile::parse_for_guild(&files.state_json, guild_id(100)).unwrap();
    let everyone = definition.roles[&logical_id("everyone")].attributes();

    assert!(everyone.name.is_none());
    assert!(everyone.color.is_none());
    assert!(everyone.hoist.is_none());
    assert!(everyone.mentionable.is_none());
    assert_eq!(literal_bool(everyone.permissions.get("VIEW_CHANNEL")), Some(true));
    assert_eq!(literal_bool(everyone.permissions.get("SEND_MESSAGES")), Some(false));
    assert!(!state.roles.contains_key(&logical_id("everyone")));

    let plan = plan_roles(&source, guild_id(100), &files.definition_toml, &files.state_json)
        .await
        .unwrap();
    assert!(plan.is_empty());
}

/// @everyoneへ名前など権限以外の管理属性を指定した定義を拒否し、Discord固有の制約を守る。
#[tokio::test]
async fn everyone_rejects_non_permission_managed_attributes() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };
    let definition = r#"
        schema_version = 1
        [roles.everyone]
        name = "renamed"
    "#;

    let error = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("@everyone") && message.contains("権限"))
    );
}

/// 再export時に既存stateの論理IDを引き継ぎ、Role名の変更で対応関係が変わらないことを保証する。
#[tokio::test]
async fn re_export_preserves_logical_ids_from_input_state() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "名称変更後")],
    };
    let previous_state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": { "moderator": "200" }
    }"#;

    let files = export_roles(&source, guild_id(100), Some(previous_state))
        .await
        .unwrap();
    let definition = parse_definition(&files.definition_toml).unwrap();
    let state = StateFile::parse_for_guild(&files.state_json, guild_id(100)).unwrap();

    assert_eq!(
        literal_string(&definition.roles[&logical_id("moderator")].attributes().name),
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
    let files = export_roles(&source, guild_id(100), None).await.unwrap();

    let plan = plan_roles(&source, guild_id(100), &files.definition_toml, &files.state_json)
        .await
        .unwrap();

    assert!(plan.is_empty());
}

/// 実構成と希望構成が一致する場合、空の Update を作らず変更なしとして扱うことを保証する。
#[tokio::test]
async fn identical_role_attributes_do_not_create_an_empty_update() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = "運営"
    "#;

    let plan = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap();

    assert!(plan.is_empty());
    assert!(plan.iter().all(|(_, change)| !change.is_update()));
}

/// 空の対応表から明示した managed Role を新規構築する計画を作り、Discord ID を推測しないことを保証する。
#[tokio::test]
async fn empty_state_plans_managed_role_creation_without_discord_id() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = "運営"
    "#;

    let plan = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan.get(&logical_id("moderator")).and_then(Change::recreated),
        Some(false)
    );
    assert!(plan.render().contains("moderator"));
    assert!(plan.render().contains("作成"));
    assert!(!plan.render().contains("Snowflake"));
}

/// 参照専用Roleは更新しないため、Botが管理不能なRoleでも参照先として使用できることを保証する。
#[tokio::test]
async fn reference_role_may_target_an_unmanageable_guild_role() {
    let mut external_role = role("200", "外部 Bot");
    external_role.manageable = false;
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![external_role],
    };
    let definition = r#"
        schema_version = 1
        [roles.external_bot]
        mode = "reference"
    "#;

    let plan = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"external_bot":"200"}"#),
    )
    .await
    .unwrap();

    assert!(plan.is_empty());
}

/// stateに対応を持たない@everyoneも、予約論理IDによって参照専用Roleとして解決できることを保証する。
#[tokio::test]
async fn everyone_role_may_be_used_as_a_reference() {
    let mut everyone = role("100", "@everyone");
    everyone.manageable = false;
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![everyone],
    };
    let definition = r#"
        schema_version = 1
        [roles.everyone]
        mode = "reference"
    "#;

    let plan = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap();

    assert!(plan.is_empty());
}

/// 権限のdefault指定が固定値ではなく、Guildの@everyoneに設定された基底権限を参照することを保証する。
#[tokio::test]
async fn permission_default_uses_everyone_role_value() {
    let mut everyone = role("100", "@everyone");
    everyone.manageable = false;
    let mut moderator = role("200", "運営");
    moderator.permissions.insert(known_permission("VIEW_CHANNEL"), false);
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![everyone, moderator],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator.permissions]
        VIEW_CHANNEL = { default = true }
    "#;

    let plan = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap();

    let attributes = plan
        .get(&logical_id("moderator"))
        .and_then(Change::attributes)
        .expect("権限変更は対象 Role の一つの Update にまとまります");
    let permission = attributes
        .permissions()
        .get(&known_permission("VIEW_CHANNEL"))
        .expect("VIEW_CHANNEL の変更が計画されます");
    assert_eq!(permission.current(), &false);
    assert_eq!(permission.desired(), &true);
}

/// export対象をBotが管理可能なRoleに限定し、管理不能なRoleを編集用定義へ混入させない。
#[tokio::test]
async fn export_omits_unmanageable_roles_but_keeps_manageable_roles() {
    let mut external_role = role("200", "外部 Bot");
    external_role.manageable = false;
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![external_role, role("201", "運営")],
    };

    let files = export_roles(&source, guild_id(100), None).await.unwrap();
    let definition = parse_definition(&files.definition_toml).unwrap();

    assert!(!definition.roles.contains_key(&logical_id("role_200")));
    assert!(definition.roles.contains_key(&logical_id("role_201")));
}

/// Role設定セットで指定した属性が合成され、実際のRoleとの差分としてplanされることを保証する。
#[tokio::test]
async fn role_settings_set_attributes_are_planned() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
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

    let plan = plan_roles(&source, guild_id(100), definition, state).await.unwrap();

    assert_eq!(plan.len(), 1);
    let attributes = plan
        .get(&logical_id("moderator"))
        .and_then(Change::attributes)
        .expect("設定セットによる変更は一つの Update にまとまります");
    let hoist = attributes.hoist().expect("hoist の変更が計画されます");
    assert_eq!(hoist.current(), &false);
    assert_eq!(hoist.desired(), &true);
}

/// 一つの Role に複数属性の差分がある場合、Role 単位の一つの Update に集約することを保証する。
#[tokio::test]
async fn multiple_role_attributes_are_grouped_in_one_change() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = "モデレーター"
        hoist = true
        mentionable = true
        [roles.moderator.permissions]
        SEND_MESSAGES = true
    "#;

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
        .expect("複数属性の差分は一つの Update に集約されます");
    assert_eq!(attributes.name().expect("name の変更").desired(), "モデレーター");
    assert_eq!(attributes.hoist().expect("hoist の変更").desired(), &true);
    assert_eq!(attributes.mentionable().expect("mentionable の変更").desired(), &true);
    assert!(
        attributes
            .permissions()
            .contains_key(&known_permission("SEND_MESSAGES"))
    );
}

/// Guild 内に対象が存在しなければ、対応 state を変更せずに bind を拒否することを保証する。
#[tokio::test]
async fn bind_rejects_a_missing_resource() {
    let source = MissingResourceSource;
    let definition = "schema_version = 1\n[roles.moderator]\nname = \"運営\"\n";
    let state = r#"{"schema_version":1,"guild_id":"100","roles":{}}"#;

    let error = bind_resource(
        &source,
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
        ManagementError::ResourceNotFound {
            resource_type: ResourceType::Role,
            discord_id: 200,
        }
    );
}

/// Channel の Role overwrite が未宣言の Role を参照していれば plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_an_undeclared_role_overwrite_target() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
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

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Role admin") && message.contains("宣言がありません"))
    );
}

/// 削除宣言した Role を Channel overwrite の対象にできないことを plan の入力検証で保証する。
#[tokio::test]
async fn plan_rejects_an_absent_role_overwrite_target() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let definition = r#"
        schema_version = 1
        [roles.admin]
        ensure = "absent"
        [channels.rules]
        type = "text"
        [channels.rules.overwrites."role:admin"]
        VIEW_CHANNEL = "allow"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {"admin": "200"},
        "channels": {"rules": "300"}
    }"#;

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Role admin") && message.contains("削除宣言"))
    );
}

/// state の削除済み Role を Channel overwrite の対象にできないことを plan の入力検証で保証する。
#[tokio::test]
async fn plan_rejects_a_deleted_role_overwrite_target() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let definition = r#"
        schema_version = 1
        [roles.admin]
        name = "管理"
        [channels.rules]
        type = "text"
        [channels.rules.overwrites."role:admin"]
        VIEW_CHANNEL = "allow"
    "#;
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": {"admin": "200"},
        "deleted_roles": ["admin"],
        "channels": {"rules": "300"}
    }"#;

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("Role admin") && message.contains("削除済み"))
    );
}

/// Channel の Member overwrite が未宣言の Member を参照していれば plan で診断することを保証する。
#[tokio::test]
async fn plan_rejects_an_undeclared_member_overwrite_target() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
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

    let error = plan_roles(&source, guild_id(100), definition, state).await.unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Member moderator") && message.contains("宣言がありません"))
    );
}

/// 複数設定セットの後勝ちと直接指定の最優先を確認し、省略属性を変更しない合成規則を保証する。
#[tokio::test]
async fn direct_attributes_override_later_settings_sets_and_omitted_attributes_are_retained() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
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
        .expect("複数属性の変更は一つの Update にまとまります");
    assert!(attributes.hoist().is_some());
    assert!(attributes.mentionable().is_some());
    assert!(attributes.name().is_none());
}

/// 各属性のdefault指定がschemaで定めた既定値へ解決され、必要な差分だけがplanされることを保証する。
#[tokio::test]
async fn default_specifiers_are_resolved_to_schema_version_values() {
    let mut actual = role("200", "運営");
    actual.color = Color::new(0x12_34_56).unwrap();
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![actual],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = { default = true }
        color = { default = true }
    "#;

    let plan = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap();

    let attributes = plan
        .get(&logical_id("moderator"))
        .and_then(Change::attributes)
        .expect("default 指定による変更が計画されます");
    let name = attributes.name().expect("name の変更が計画されます");
    assert_eq!(name.current(), "運営");
    assert_eq!(name.desired(), "new role");
    let color = attributes.color().expect("color の変更が計画されます");
    assert_eq!(color.current().get(), 0x12_34_56);
    assert_eq!(color.desired().get(), 0);
}

/// 新規 Role の plan も apply と同じ Discord API の具体的な default 値を表示する。
#[tokio::test]
async fn role_create_plan_resolves_default_values() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        name = { default = true }
        color = { default = true }
        hoist = { default = true }
        mentionable = { default = true }
        [roles.moderator.permissions]
        VIEW_CHANNEL = { default = true }
    "#;

    let plan = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap();
    let rendered = plan.render();

    assert!(rendered.contains("name: \"new role\""));
    assert!(rendered.contains("color: 0"));
    assert!(rendered.contains("hoist: false"));
    assert!(rendered.contains("mentionable: false"));
    assert!(rendered.contains("VIEW_CHANNEL: true"));
    assert!(!rendered.contains("Default"));
}

/// stateのGuildが要求先と異なる場合、Discordへ問い合わせる前に誤投入として拒否することを保証する。
#[tokio::test]
async fn guild_mismatch_is_reported_before_reading_discord() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let error = plan_roles(&source, guild_id(100), "schema_version = 1", &state("999", "{}"))
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
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let error = plan_roles(
        &source,
        guild_id(100),
        "schema_version = 1",
        &state("not-a-snowflake", "{}"),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("Guild ID")));
}

/// 予約論理ID everyoneをstateに保存する旧・不正形式を、stateモデルの検証で拒否することを保証する。
#[tokio::test]
async fn everyone_mapping_in_state_is_rejected() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };
    let error = plan_roles(
        &source,
        guild_id(100),
        "schema_version = 1\n[roles.everyone]",
        &state("100", r#"{"everyone":"100"}"#),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("everyone") && message.contains("state に含めず"))
    );
}

/// @everyone の実 ID を別の論理 ID に保存した state も拒否することを保証する。
#[tokio::test]
async fn everyone_role_id_under_another_logical_id_is_rejected() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("100", "@everyone")],
    };
    let error = plan_roles(
        &source,
        guild_id(100),
        "schema_version = 1\n[roles.default_role]\nmode = \"reference\"",
        &state("100", r#"{"default_role":"100"}"#),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidState(message) if message.contains("everyone") && message.contains("別の論理 ID"))
    );
}

/// 複数の論理IDが同じRole Snowflakeを指す曖昧なstateを拒否することを保証する。
#[tokio::test]
async fn duplicate_snowflakes_in_state_are_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let error = export_roles(
        &source,
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
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let state = r#"{
        "schema_version": 1,
        "guild_id": "100",
        "roles": { "moderator": "200", "moderator": "201" }
    }"#;

    let error = export_roles(&source, guild_id(100), Some(state)).await.unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("論理 ID moderator が重複")));
}

/// 新規Role用に生成する論理IDが古いstateの予約と衝突した場合、誤対応せず拒否することを保証する。
#[tokio::test]
async fn generated_logical_id_collision_with_stale_state_is_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let error = export_roles(&source, guild_id(100), Some(&state("100", r#"{"role_200":"999"}"#)))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidState(message) if message.contains("論理 ID role_200")));
}

/// タイプミスなどの未知フィールドを無視せず、definitionの入力エラーとして報告することを保証する。
#[tokio::test]
async fn unknown_definition_keys_are_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        unknown = true
    "#;

    let error = plan_roles(
        &source,
        guild_id(100),
        definition,
        &state("100", r#"{"moderator":"200"}"#),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(_)));
}

/// Roleから未参照の設定セットも検証し、潜在的な不正定義を見逃さないことを保証する。
#[tokio::test]
async fn invalid_unreferenced_settings_set_is_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let definition = r#"
        schema_version = 1
        [settings_sets.role.unused]
        color = 16777216
    "#;

    let error = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("color")));
}

/// 未参照設定セット内でも無効なdefault指定を拒否し、定義全体を一貫して検証する。
#[tokio::test]
async fn false_default_in_unreferenced_settings_set_is_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let definition = r#"
        schema_version = 1
        [settings_sets.role.unused]
        hoist = { default = false }
    "#;

    let error = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("default")));
}

/// 未参照設定セット内の未知権限も検出し、将来参照された際の遅延エラーを防ぐ。
#[tokio::test]
async fn unknown_permission_in_unreferenced_settings_set_is_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: vec![role("200", "運営")],
    };
    let definition = r#"
        schema_version = 1
        [settings_sets.role.unused.permissions]
        NOT_A_DISCORD_PERMISSION = true
    "#;

    let error = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未知の権限")));
}

/// 未知権限はconfigurationのresolve段階で拒否し、Role catalogやbuild_planへ到達させないことを保証する。
#[tokio::test]
async fn unknown_permission_is_rejected_before_catalog_and_build_plan() {
    let definition = r#"
        schema_version = 1
        [roles.moderator.permissions]
        NOT_A_DISCORD_PERMISSION = true
    "#;

    let error = plan_roles(&CatalogMustNotBeRead, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未知の権限")));
}

/// 対応外schema_versionのdefinitionを拒否し、異なる解釈でRoleを更新しないことを保証する。
#[tokio::test]
async fn unsupported_definition_version_is_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };

    let error = plan_roles(&source, guild_id(100), "schema_version = 2", &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("schema_version")));
}

/// 利用者向けサンプルTOMLが現在のDefinitionFileとして読み取れる状態を維持する。
#[test]
fn editor_sample_matches_the_supported_definition_shape() {
    let sample = include_str!("../../../../../docs/examples/discord-role-management.toml");
    parse_definition(sample).unwrap();
}

/// Roleが存在しない設定セットを参照した場合、合成処理へ進む前に定義エラーとして報告する。
#[tokio::test]
async fn unknown_settings_set_is_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let definition = r#"
        schema_version = 1
        [roles.moderator]
        settings_sets = ["missing"]
    "#;

    let error = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("未知の設定セット")));
}

/// 参照専用Roleへの管理属性指定を拒否し、更新されない設定を利用者に誤認させない。
#[tokio::test]
async fn reference_role_with_managed_attributes_is_reported() {
    let source = StatefulFakeRoleSource {
        guild_id: "100".to_owned(),
        roles: Vec::new(),
    };
    let definition = r#"
        schema_version = 1
        [roles.external]
        mode = "reference"
        hoist = true
    "#;

    let error = plan_roles(&source, guild_id(100), definition, &state("100", "{}"))
        .await
        .unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("参照専用 Role")));
}

/// Channel が未宣言の設定セットを参照した場合、Raw属性として残さずparseで拒否する。
#[test]
fn channel_rejects_an_unknown_settings_set_while_parsing() {
    let definition = r#"
        schema_version = 1
        [channels.rules]
        type = "text"
        settings_sets = ["missing"]
    "#;

    let error = parse_definition(definition).unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Channel rules") && message.contains("未知の設定セット missing"))
    );
}

/// Channel の設定セット参照は順序付きかつ重複しない値としてparseする。
#[test]
fn channel_rejects_duplicate_settings_sets_while_parsing() {
    let definition = r#"
        schema_version = 1
        [settings_sets.channel.readonly]
        name = "閲覧専用"
        [channels.rules]
        type = "text"
        settings_sets = ["readonly", "readonly"]
    "#;

    let error = parse_definition(definition).unwrap_err();

    assert!(
        matches!(error, ManagementError::InvalidDefinition(message) if message.contains("Channel rules") && message.contains("重複"))
    );
}

/// Channel の設定セット参照順に従い、後段の設定セットが前段の属性を上書きする。
#[test]
fn channel_parses_ordered_settings_sets() {
    let definition = r#"
        schema_version = 1
        [settings_sets.channel.readonly]
        name = "閲覧専用"
        [settings_sets.channel.writable]
        name = "書き込み可能"
        [channels.rules]
        type = "text"
        settings_sets = ["readonly", "writable"]
    "#;

    let definition = parse_definition(definition).unwrap();
    assert_eq!(
        definition.channels[&ChannelLogicalId::parse("rules").unwrap()]
            .attributes()
            .name
            .as_ref(),
        Some(&ChannelValue::Value("書き込み可能".to_owned()))
    );
}

/// 参照専用 Channel は設定セットを持てず、属性管理との区別を型変換時に確立する。
#[test]
fn reference_channel_rejects_settings_sets_while_parsing() {
    let definition = r#"
        schema_version = 1
        [settings_sets.channel.readonly]
        name = "閲覧専用"
        [channels.rules]
        mode = "reference"
        settings_sets = ["readonly"]
    "#;

    let error = parse_definition(definition).unwrap_err();

    assert!(matches!(error, ManagementError::InvalidDefinition(message) if message.contains("参照専用 Channel rules")));
}

#[cfg(test)]
#[path = "tests/apply_tests.rs"]
mod apply_tests;

#[cfg(test)]
#[path = "tests/channel_tests.rs"]
mod channel_tests;
