use super::presentation;
use crate::{
    app::AppError,
    utils::{create_ephemeral_message, create_safe_allowed_mentions},
};
use serenity::{
    all::{ComponentInteraction, Context, MessageFlags, RoleId},
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
};

pub(in crate::features::mcguildlink::links) async fn show_page(
    ctx: &Context,
    component: &ComponentInteraction,
    moderator_role: RoleId,
    snapshot_id: u64,
    page: usize,
) -> Result<(), AppError> {
    let Some(snapshot) = presentation::get(snapshot_id) else {
        component
            .create_response(
                &ctx.http,
                create_ephemeral_message("一覧の有効期限が切れました。もう一度開き直してください。", None),
            )
            .await?;
        return Ok(());
    };
    if snapshot.owner != component.user.id.get()
        || !component
            .member
            .as_ref()
            .is_some_and(|member| member.roles.contains(&moderator_role))
    {
        component
            .create_response(&ctx.http, create_ephemeral_message("管理者権限が必要です。", None))
            .await?;
        return Ok(());
    }
    component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .flags(MessageFlags::IS_COMPONENTS_V2)
                    .components(presentation::render(snapshot_id, &snapshot, page))
                    .allowed_mentions(create_safe_allowed_mentions()),
            ),
        )
        .await?;
    Ok(())
}
