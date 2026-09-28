use super::{
    ports::AccountLinksRepository,
    presentation::{Scope, get_snapshot, load, page, save_snapshot},
};
use crate::{
    app::AppError,
    utils::{create_ephemeral_message, create_safe_allowed_mentions},
};
use serenity::{
    all::{
        ComponentInteraction, Context, EditInteractionResponse, LabelComponent, ModalComponent, ModalInteraction,
        RoleId,
    },
    builder::{
        CreateCheckbox, CreateInteractionResponse, CreateInteractionResponseMessage, CreateLabel, CreateModal,
        CreateModalComponent, CreateTextDisplay,
    },
};
use uuid::Uuid;

pub(super) const UNLINK_BUTTON_PREFIX: &str = "unlink_button:";
pub(super) const UNLINK_MODAL_PREFIX: &str = "unlink_confirm_modal:";
const CONFIRM_ID: &str = "unlink_confirm_checkbox";

pub(super) fn parse_unlink(id: &str, prefix: &str) -> Option<(u64, Uuid)> {
    let (owner, uuid) = id.strip_prefix(prefix)?.split_once(':')?;
    Some((owner.parse().ok()?, Uuid::parse_str(uuid).ok()?))
}

pub(super) async fn show_link_list(
    store: &impl AccountLinksRepository,
    ctx: &Context,
    component: &ComponentInteraction,
) -> Result<(), AppError> {
    component.defer_ephemeral(&ctx.http).await?;
    let scope = Scope::User(component.user.id.get());
    let links = load(store, scope).await?;
    let snapshot = save_snapshot(component.id.get(), component.user.id.get(), scope, links);
    let (content, components) = page(component.id.get(), &snapshot, 0);
    component
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new()
                .content(content)
                .components(components)
                .allowed_mentions(create_safe_allowed_mentions()),
        )
        .await?;

    Ok(())
}

pub(super) async fn show_page(
    ctx: &Context,
    component: &ComponentInteraction,
    moderator_role: RoleId,
    snapshot_id: u64,
    page_index: usize,
) -> Result<(), AppError> {
    let snapshot = get_snapshot(snapshot_id);
    if snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.owner != component.user.id.get())
    {
        component
            .create_response(
                &ctx.http,
                create_ephemeral_message("不正な操作です。このボタンはあなたのものではありません。", None),
            )
            .await?;
        return Ok(());
    }
    if snapshot.as_ref().is_some_and(|snapshot| snapshot.scope.is_admin())
        && !component
            .member
            .as_ref()
            .is_some_and(|member| member.roles.contains(&moderator_role))
    {
        component
            .create_response(&ctx.http, create_ephemeral_message("管理者権限が必要です。", None))
            .await?;
        return Ok(());
    }
    let (content, components) = match snapshot {
        Some(snapshot) => page(snapshot_id, &snapshot, page_index),
        None => (
            "一覧の有効期限が切れました。もう一度開き直してください。".into(),
            Vec::new(),
        ),
    };
    component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .components(components)
                    .allowed_mentions(create_safe_allowed_mentions()),
            ),
        )
        .await?;

    Ok(())
}

pub(super) async fn show_unlink_confirmation(
    store: &impl AccountLinksRepository,
    ctx: &Context,
    component: &ComponentInteraction,
    owner: u64,
    uuid: Uuid,
) -> Result<(), AppError> {
    if owner != component.user.id.get() {
        component
            .create_response(
                &ctx.http,
                create_ephemeral_message("不正な操作です。このボタンはあなたのものではありません。", None),
            )
            .await?;
        return Ok(());
    }
    let link = store
        .by_discord(owner)
        .await?
        .into_iter()
        .find(|link| link.minecraft_uuid == uuid);
    let Some(link) = link else {
        component
            .create_response(
                &ctx.http,
                create_ephemeral_message(
                    "アカウント情報を取得できませんでした。すでに解除されている可能性があります。",
                    None,
                ),
            )
            .await?;
        return Ok(());
    };
    component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Modal(
                CreateModal::new(format!("{UNLINK_MODAL_PREFIX}{owner}:{uuid}"), "アカウントの紐付け解除").components(
                    vec![
                        CreateModalComponent::TextDisplay(CreateTextDisplay::new(format!(
                            "本当に **{}** との紐付けを解除しますか？ この操作は取り消せません。",
                            link.minecraft_name,
                        ))),
                        CreateModalComponent::Label(CreateLabel::checkbox(
                            "内容を確認し、解除に同意します。",
                            CreateCheckbox::new(CONFIRM_ID).default_selected(false),
                        )),
                    ],
                ),
            ),
        )
        .await?;

    Ok(())
}

pub(super) async fn complete_unlink(
    store: &impl AccountLinksRepository,
    ctx: &Context,
    modal: &ModalInteraction,
    owner: u64,
    uuid: Uuid,
) -> Result<(), AppError> {
    if owner != modal.user.id.get() {
        modal
            .create_response(
                &ctx.http,
                create_ephemeral_message("不正な操作です。このモーダルはあなたのものではありません。", None),
            )
            .await?;
        return Ok(());
    }
    let confirmed = modal.data.components.iter().any(|component| {
        matches!(component, ModalComponent::Label(label)
                    if matches!(&label.component, LabelComponent::Checkbox(checkbox)
                        if checkbox.custom_id == CONFIRM_ID && checkbox.value))
    });
    if !confirmed {
        modal.create_response(&ctx.http, create_ephemeral_message("紐付けの解除をキャンセルしました。解除する場合は、内容を確認し、チェックボックスに同意してください。", None)).await?;
        return Ok(());
    }
    let removed = store.unlink(owner, uuid).await?;
    let content = if removed {
        "アカウントの紐付けを解除しました。"
    } else {
        "アカウント情報を取得できませんでした。すでに解除されている可能性があります。"
    };
    modal
        .create_response(&ctx.http, create_ephemeral_message(content, None))
        .await?;
    Ok(())
}
