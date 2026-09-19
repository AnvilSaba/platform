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
    service::{ManagementError, RoleApplyResult, RoleApplyStatus, RoleManagementService, RolePlan},
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
        RoleApplyStatus::Failed(error) => format!("Role の変更中に失敗したため停止しました: {error}"),
        RoleApplyStatus::ResponseUnknown => {
            "Role 更新の応答を確認できず、再取得した値も希望値と一致しないため停止しました。".to_owned()
        }
    };
    format!(
        "{summary}\n成功した属性: {}\n未完了の属性: {}",
        result.applied.len(),
        result.pending.len()
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

    let confirmations = ConfirmationStore::default();
    let token = confirmations.issue(
        ctx.author().id.get(),
        PendingRoleApply {
            guild_id,
            definition: definition_text,
            state: state_text,
            plan,
        },
        Instant::now(),
    );
    let custom_id = token.custom_id();

    ctx.send(
        CreateReply::default()
            .content("添付の変更計画を確認し、5分以内に適用してください。")
            .attachment(CreateAttachment::bytes(rendered_plan, "discord-role-plan.txt"))
            .components(&[CreateComponent::ActionRow(CreateActionRow::buttons(&[
                CreateButton::new(&custom_id)
                    .label("Role の変更を適用")
                    .style(ButtonStyle::Danger),
            ]))]),
    )
    .await?;

    let custom_ids: FixedArray<FixedString> = vec![FixedString::from_string_trunc(custom_id)].try_into().unwrap();
    let mut interactions = ComponentInteractionCollector::new(ctx.serenity_context())
        .custom_ids(custom_ids)
        .timeout(CONFIRMATION_WINDOW)
        .stream();
    while let Some(interaction) = interactions.next().await {
        interaction.defer_ephemeral(ctx.http()).await?;
        match confirmations.consume(&token, interaction.user.id.get(), Instant::now()) {
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
                let result = service
                    .apply_roles(
                        payload.guild_id,
                        &payload.definition,
                        &payload.state,
                        &payload.plan,
                        deadlines.processing,
                    )
                    .await;
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
                return Ok(());
            }
        }
    }

    Ok(())
}
