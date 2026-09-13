use std::time::{Duration, Instant};

use futures::StreamExt as _;
use poise::CreateReply;
use serenity::{
    all::{
        Attachment, ButtonStyle, ComponentInteractionCollector, CreateActionRow, CreateButton, EditInteractionResponse,
    },
    builder::{CreateAttachment, CreateComponent},
    small_fixed_array::{FixedArray, FixedString},
};
use tracing::warn;

use crate::app::{AppContext, AppError};

use super::{
    adapter::SerenityRoleSource,
    confirmation::{ConfirmationError, ConfirmationStore},
    ids::GuildId,
    service::{
        ManagementError, ResourceType, RoleApplyOptions, RoleApplyResult, RoleApplyStatus, RoleLifecycleChange,
        RoleManagementService, RolePlan,
    },
};

const CONFIRMATION_WINDOW: Duration = Duration::from_secs(5 * 60);
const APPLY_PROCESSING_BUDGET: Duration = Duration::from_secs(10 * 60);
const APPLY_RESULT_BUDGET: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Copy)]
struct ApplyDeadlines {
    processing: Instant,
    response: Instant,
}

fn apply_deadlines(started_at: Instant) -> ApplyDeadlines {
    let processing = started_at + APPLY_PROCESSING_BUDGET;
    ApplyDeadlines {
        processing,
        response: processing + APPLY_RESULT_BUDGET,
    }
}

#[derive(Clone)]
struct PendingRoleApply {
    guild_id: GuildId,
    definition: String,
    state: String,
    plan: RolePlan,
}

fn render_apply_result(result: &RoleApplyResult) -> String {
    let summary = match &result.status {
        RoleApplyStatus::Complete => "Role の変更を適用しました。".to_owned(),
        RoleApplyStatus::GuildBusy => "同じ Guild の別の apply が進行中です。".to_owned(),
        RoleApplyStatus::ReplanRequired => {
            "確認後に管理属性が変化しました。新しい plan を確認してください。".to_owned()
        }
        RoleApplyStatus::DeadlineExceeded => "処理期限に達したため、新しい変更を開始せず停止しました。".to_owned(),
        RoleApplyStatus::DeletionPermissionRequired => "削除を含むため、削除許可付きの確認が必要です。".to_owned(),
        RoleApplyStatus::DeletionPermissionDenied(error) => {
            format!("Discord の Role 削除権限が不足しているため停止しました: {error}")
        }
        RoleApplyStatus::DeletionVerificationPermissionDenied(error) => {
            format!("削除後の存在確認に必要な権限が不足しているため停止しました: {error}")
        }
        RoleApplyStatus::CreationResponseUnknown => {
            "Role 作成の応答を確認できませんでした。重複作成を避けるため、state の確認が必要です。".to_owned()
        }
        RoleApplyStatus::DeletionResponseUnknown => {
            "Role 削除の応答を確認できませんでした。既知の ID と削除意図を保持して停止しました。".to_owned()
        }
        RoleApplyStatus::DeletionVerificationIndeterminate(error) => {
            format!("Role 削除後の存在確認が判定不能なため停止しました。削除済みとは扱いません: {error}")
        }
        RoleApplyStatus::Failed(error) => format!("Role の変更中に失敗したため停止しました: {error}"),
        RoleApplyStatus::ResponseUnknown => {
            "Role 更新の応答を確認できず、再取得した値も希望値と一致しないため停止しました。".to_owned()
        }
    };
    format!(
        "{summary}\n成功した属性: {} / Role 操作: {}\n未完了の属性: {} / Role 操作: {}",
        result.applied.len(),
        result.applied_lifecycle.len(),
        result.pending.len(),
        result.pending_lifecycle.len(),
    )
}

async fn read_text(attachment: &Attachment) -> Result<String, ManagementError> {
    let bytes = attachment.download().await.map_err(|error| {
        ManagementError::InvalidInputFile(format!("{} を取得できません: {error}", attachment.filename))
    })?;
    String::from_utf8(bytes).map_err(|error| {
        ManagementError::InvalidInputFile(format!(
            "{} は UTF-8 テキストではありません: {error}",
            attachment.filename
        ))
    })
}

async fn send_input_error(ctx: AppContext<'_>, error: ManagementError) -> Result<(), AppError> {
    ctx.send(CreateReply::default().content(format!("入力を確認してください。\n{error}")))
        .await?;
    Ok(())
}

/// 作成結果が不明な既存 Resource を、所有者が確認した Discord ID と state に bind します。
#[poise::command(slash_command, ephemeral, guild_only, owners_only)]
pub async fn bind(
    ctx: AppContext<'_>,
    #[description = "希望構成の TOML"] definition: Attachment,
    #[description = "現在の対応 state JSON"] state: Attachment,
    #[description = "role、channel、member のいずれか"] resource_type: String,
    #[description = "definition にある論理 ID"] logical_id: String,
    #[description = "所有者が確認した Discord ID"] discord_id: String,
) -> Result<(), AppError> {
    ctx.defer_ephemeral().await?;
    let definition_text = match read_text(&definition).await {
        Ok(text) => text,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let state_text = match read_text(&state).await {
        Ok(text) => text,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let resource_type = match resource_type.parse::<ResourceType>() {
        Ok(resource_type) => resource_type,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let guild_id = GuildId::from(ctx.guild_id().expect("guild_only command"));
    let source = SerenityRoleSource::new(ctx.http(), ctx.cache().current_user().id);
    let service = RoleManagementService::new(source);

    match service
        .bind_resource(
            guild_id,
            &definition_text,
            &state_text,
            resource_type,
            &logical_id,
            &discord_id,
        )
        .await
    {
        Ok(result) => {
            ctx.send(
                CreateReply::default()
                    .content(
                        "対応 state を更新しました。作成結果が不明な Resource は Discord 上で ID を確認してから bind してください。",
                    )
                    .attachment(CreateAttachment::bytes(result.state_json, "discord-state.json")),
            )
            .await?;
            Ok(())
        }
        Err(error) => send_input_error(ctx, error).await,
    }
}

/// 管理可能な Role の現在値と対応 state を出力します。
#[poise::command(
    slash_command,
    ephemeral,
    guild_only,
    owners_only,
    required_bot_permissions = "MANAGE_ROLES"
)]
pub async fn role_export(
    ctx: AppContext<'_>,
    #[description = "再 export で論理 ID を維持する state JSON"] state: Option<Attachment>,
) -> Result<(), AppError> {
    ctx.defer_ephemeral().await?;
    let state_text = match state.as_ref() {
        Some(attachment) => match read_text(attachment).await {
            Ok(text) => Some(text),
            Err(error) => return send_input_error(ctx, error).await,
        },
        None => None,
    };
    let guild_id = GuildId::from(ctx.guild_id().expect("guild_only command"));
    let source = SerenityRoleSource::new(ctx.http(), ctx.cache().current_user().id);
    let service = RoleManagementService::new(source);

    match service.export_roles(guild_id, state_text.as_deref()).await {
        Ok(files) => {
            ctx.send(
                CreateReply::default()
                    .content("Role の定義と state を出力しました。")
                    .attachment(CreateAttachment::bytes(files.definition_toml, "discord-roles.toml"))
                    .attachment(CreateAttachment::bytes(files.state_json, "discord-state.json")),
            )
            .await?;
            Ok(())
        }
        Err(error) => send_input_error(ctx, error).await,
    }
}

/// Role 定義と実構成を比較し、属性単位の変更計画を出力します。
#[poise::command(
    slash_command,
    ephemeral,
    guild_only,
    owners_only,
    required_bot_permissions = "MANAGE_ROLES"
)]
pub async fn role_plan(
    ctx: AppContext<'_>,
    #[description = "希望構成の TOML"] definition: Attachment,
    #[description = "対象 Guild の state JSON"] state: Attachment,
) -> Result<(), AppError> {
    ctx.defer_ephemeral().await?;
    let definition_text = match read_text(&definition).await {
        Ok(text) => text,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let state_text = match read_text(&state).await {
        Ok(text) => text,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let guild_id = GuildId::from(ctx.guild_id().expect("guild_only command"));
    let source = SerenityRoleSource::new(ctx.http(), ctx.cache().current_user().id);
    let service = RoleManagementService::new(source);

    match service.plan_roles(guild_id, &definition_text, &state_text).await {
        Ok(plan) => {
            ctx.send(
                CreateReply::default()
                    .content("Role の変更計画を出力しました。")
                    .attachment(CreateAttachment::bytes(plan.render(), "discord-role-plan.txt")),
            )
            .await?;
            Ok(())
        }
        Err(error) => send_input_error(ctx, error).await,
    }
}

/// Role の変更計画を確認後、一度限りのボタン操作で適用します。
#[poise::command(
    slash_command,
    ephemeral,
    guild_only,
    owners_only,
    required_bot_permissions = "MANAGE_ROLES"
)]
pub async fn role_apply(
    ctx: AppContext<'_>,
    #[description = "希望構成の TOML"] definition: Attachment,
    #[description = "対象 Guild の state JSON"] state: Attachment,
) -> Result<(), AppError> {
    ctx.defer_ephemeral().await?;
    let definition_text = match read_text(&definition).await {
        Ok(text) => text,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let state_text = match read_text(&state).await {
        Ok(text) => text,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let guild_id = GuildId::from(ctx.guild_id().expect("guild_only command"));
    let source = SerenityRoleSource::new(ctx.http(), ctx.cache().current_user().id);
    let service = RoleManagementService::new(source);
    let plan = match service.plan_roles(guild_id, &definition_text, &state_text).await {
        Ok(plan) => plan,
        Err(error) => return send_input_error(ctx, error).await,
    };
    let rendered_plan = plan.render();
    let deletion_in_plan = plan
        .lifecycle
        .iter()
        .any(|change| matches!(change, RoleLifecycleChange::Delete { .. }));

    let confirmations = ConfirmationStore::default();
    let pending = PendingRoleApply {
        guild_id,
        definition: definition_text,
        state: state_text,
        plan,
    };
    let token = confirmations.issue(ctx.author().id.get(), pending.clone(), Instant::now());
    let deletion_token = deletion_in_plan.then(|| confirmations.issue(ctx.author().id.get(), pending, Instant::now()));
    let custom_id = token.custom_id();
    let deletion_custom_id = deletion_token.as_ref().map(|token| token.custom_id());
    let mut buttons = vec![
        CreateButton::new(&custom_id)
            .label("Role の変更を適用")
            .style(ButtonStyle::Primary),
    ];
    if let Some(custom_id) = &deletion_custom_id {
        buttons.push(
            CreateButton::new(custom_id)
                .label("削除を許可して適用")
                .style(ButtonStyle::Danger),
        );
    }

    ctx.send(
        CreateReply::default()
            .content("添付の変更計画を確認し、5分以内に適用してください。")
            .attachment(CreateAttachment::bytes(rendered_plan, "discord-role-plan.txt"))
            .components(&[CreateComponent::ActionRow(CreateActionRow::buttons(&buttons))]),
    )
    .await?;

    let custom_ids: FixedArray<FixedString> = std::iter::once(custom_id.clone())
        .chain(deletion_custom_id.clone())
        .map(FixedString::from_string_trunc)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let mut interactions = ComponentInteractionCollector::new(ctx.serenity_context())
        .custom_ids(custom_ids)
        .timeout(CONFIRMATION_WINDOW)
        .stream();
    while let Some(interaction) = interactions.next().await {
        interaction.defer_ephemeral(ctx.http()).await?;
        let allow_deletions = deletion_custom_id
            .as_deref()
            .is_some_and(|custom_id| custom_id == interaction.data.custom_id);
        let selected_token = if allow_deletions {
            deletion_token.as_ref().expect("削除ボタンには削除用トークンがあります")
        } else {
            &token
        };
        match confirmations.consume(selected_token, interaction.user.id.get(), Instant::now()) {
            Err(ConfirmationError::WrongOwner) => {
                interaction
                    .edit_response(
                        ctx.http(),
                        EditInteractionResponse::new().content("この確認ボタンは plan の作成者だけが操作できます。"),
                    )
                    .await?;
            }
            Err(ConfirmationError::Expired) => {
                interaction
                    .edit_response(
                        ctx.http(),
                        EditInteractionResponse::new()
                            .content("確認ボタンは失効しました。もう一度 plan を作成してください。"),
                    )
                    .await?;
                return Ok(());
            }
            Err(ConfirmationError::AlreadyConsumed | ConfirmationError::Unknown) => {
                interaction
                    .edit_response(
                        ctx.http(),
                        EditInteractionResponse::new().content("この確認ボタンはすでに使用されています。"),
                    )
                    .await?;
                return Ok(());
            }
            Ok(payload) => {
                let deadlines = apply_deadlines(Instant::now());
                let source = SerenityRoleSource::new(ctx.http(), ctx.cache().current_user().id);
                let service = RoleManagementService::new(source);
                let result = if allow_deletions {
                    service
                        .apply_roles_with_options(
                            payload.guild_id,
                            &payload.definition,
                            &payload.state,
                            &payload.plan,
                            RoleApplyOptions { allow_deletions: true },
                            deadlines.processing,
                        )
                        .await
                } else {
                    service
                        .apply_roles(
                            payload.guild_id,
                            &payload.definition,
                            &payload.state,
                            &payload.plan,
                            deadlines.processing,
                        )
                        .await
                };
                let needs_deletion_confirmation = matches!(
                    &result,
                    Ok(result) if result.status == RoleApplyStatus::DeletionPermissionRequired
                );
                let edit = match result {
                    Ok(result) => EditInteractionResponse::new()
                        .content(render_apply_result(&result))
                        .new_attachment(CreateAttachment::bytes(result.state_json, "discord-state.json")),
                    Err(error) => EditInteractionResponse::new().content(format!("入力を確認してください。\n{error}")),
                };
                match tokio::time::timeout(
                    deadlines.response.saturating_duration_since(Instant::now()),
                    interaction.edit_response(ctx.http(), edit),
                )
                .await
                {
                    Ok(response) => {
                        response?;
                    }
                    Err(_) => warn!("Role apply result response exceeded the two-minute return budget"),
                }
                if needs_deletion_confirmation {
                    continue;
                }
                return Ok(());
            }
        }
    }

    Ok(())
}
