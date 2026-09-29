use super::presentation;
use crate::{
    app::AppError,
    utils::{create_ephemeral_message, create_safe_allowed_mentions},
};
use serenity::{
    all::{ComponentInteraction, Context, MessageFlags},
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
};

pub(in crate::features::mcguildlink::links) async fn show_page(
    ctx: &Context,
    component: &ComponentInteraction,
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
    if snapshot.owner != component.user.id.get() {
        component
            .create_response(
                &ctx.http,
                create_ephemeral_message("不正な操作です。このボタンはあなたのものではありません。", None),
            )
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
