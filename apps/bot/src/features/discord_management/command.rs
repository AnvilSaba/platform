use poise::CreateReply;
use serenity::{all::Attachment, builder::CreateAttachment};

use crate::app::{AppContext, AppError};

use super::{
    adapter::SerenityRoleSource,
    ids::GuildId,
    service::{ManagementError, RoleManagementService},
};

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
