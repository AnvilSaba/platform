use super::{
    discord::LIST_LINK_BUTTON_ID,
    management_store::{Link, LinkManagement},
};
use crate::{
    app::{AppApplicationContext, AppError, BotDataExt, BotError},
    utils::{create_ephemeral_message, create_safe_allowed_mentions},
};
use bot_macros::event_handler;
use poise::CreateReply;
use serenity::{
    all::{
        ButtonStyle, ComponentInteractionDataKind, Context, CreateActionRow, CreateButton, EditInteractionResponse,
        FullEvent, Interaction, LabelComponent, ModalComponent, User,
    },
    builder::{
        CreateCheckbox, CreateComponent, CreateInteractionResponse, CreateLabel, CreateModal, CreateModalComponent,
        CreateTextDisplay,
    },
};
use uuid::Uuid;

const PAGE_SIZE: usize = 5;
const CONFIRM_ID: &str = "unlink_confirm_checkbox";

#[derive(Clone, Copy)]
enum Scope {
    User(u64),
    Discord(u64),
    Minecraft(Uuid),
    All,
}

impl Scope {
    fn custom_id(self, owner: u64, page: usize) -> String {
        match self {
            Self::User(_) => format!("ml:u:{owner}:{page}"),
            Self::Discord(target) => format!("ml:d:{owner}:{target}:{page}"),
            Self::Minecraft(uuid) => format!("ml:m:{owner}:{uuid}:{page}"),
            Self::All => format!("ml:a:{owner}:{page}"),
        }
    }

    fn is_admin(self) -> bool {
        !matches!(self, Self::User(_))
    }
}

async fn load(store: &LinkManagement, scope: Scope) -> Result<Vec<Link>, AppError> {
    match scope {
        Scope::User(id) | Scope::Discord(id) => store.by_discord(id).await,
        Scope::Minecraft(uuid) => store.by_minecraft(uuid).await,
        Scope::All => store.all().await,
    }
}

fn page(scope: Scope, owner: u64, links: &[Link], requested: usize) -> (String, Vec<CreateComponent<'static>>) {
    if links.is_empty() {
        let empty = match scope {
            Scope::User(_) => "あなたの Discordアカウントに紐付けられた Minecraftアカウントはありません。",
            Scope::Discord(_) => "その Discordアカウントに紐付けられている Minecraftアカウントはありません。",
            Scope::Minecraft(_) => "その Minecraftアカウントに紐付けられている Discordアカウントはありません。",
            Scope::All => "紐付け済みアカウントはありません。",
        };
        return (empty.into(), Vec::new());
    }

    let pages = links.len().div_ceil(PAGE_SIZE);
    let index = requested.min(pages - 1);
    let title = match scope {
        Scope::User(_) => {
            "## 紐付けられたアカウント\n以下の Minecraftアカウントがあなたの Discordアカウントに紐付けられています。\n解除するには各アカウントのボタンを押してください。"
        }
        Scope::Discord(_) => "## 紐付け一覧 (Discord)",
        Scope::Minecraft(_) => "## 紐付け一覧 (Minecraft)",
        Scope::All => "## 紐付け一覧 (All)",
    };
    let mut content = format!("{title}\n\n");
    let mut unlink_buttons = Vec::new();
    for (item, link) in links.iter().skip(index * PAGE_SIZE).take(PAGE_SIZE).enumerate() {
        if scope.is_admin() {
            content.push_str(&format!(
                "{}. Discord: <@{}> (`{}`)\nMinecraft: **{}** (`{}`)\n紐付け日時: <t:{}:F>\n\n",
                index * PAGE_SIZE + item + 1,
                link.discord_user_id,
                link.discord_name,
                link.minecraft_name,
                link.minecraft_uuid,
                link.linked_at.timestamp(),
            ));
        } else {
            content.push_str(&format!(
                "{}. **{}** (`{}`)\n",
                index * PAGE_SIZE + item + 1,
                link.minecraft_name,
                link.minecraft_uuid,
            ));
            unlink_buttons.push(
                CreateButton::new(format!("ml:x:{owner}:{}", link.minecraft_uuid))
                    .label(format!("{} を解除", item + 1))
                    .style(ButtonStyle::Danger),
            );
        }
    }
    content.push_str(&format!("ページ {}/{}", index + 1, pages));
    let mut components = Vec::new();
    if !unlink_buttons.is_empty() {
        components.push(CreateComponent::ActionRow(CreateActionRow::buttons(unlink_buttons)));
    }
    if pages > 1 {
        let navigation = vec![
            CreateButton::new(scope.custom_id(owner, index.saturating_sub(1)))
                .label("前へ")
                .style(ButtonStyle::Secondary)
                .disabled(index == 0),
            CreateButton::new(scope.custom_id(owner, index + 1))
                .label("次へ")
                .style(ButtonStyle::Secondary)
                .disabled(index + 1 >= pages),
        ];
        components.push(CreateComponent::ActionRow(CreateActionRow::buttons(navigation)));
    }
    (content, components)
}

async fn moderator(ctx: AppApplicationContext<'_>) -> Result<bool, AppError> {
    let config = ctx.app_config().await;
    Ok(ctx.guild_id() == Some(config.mcguildlink.guild_id)
        && ctx
            .interaction
            .member
            .as_ref()
            .is_some_and(|member| member.roles.contains(&config.mcguildlink.moderator_role_id)))
}

async fn send_command_page(ctx: AppApplicationContext<'_>, scope: Scope) -> Result<(), AppError> {
    if !moderator(ctx).await? {
        return Err(BotError::HasNoRole.into());
    }
    ctx.defer_ephemeral().await?;
    let store = LinkManagement::new(ctx.bot_data().database.clone());
    let links = load(&store, scope).await?;
    let (content, components) = page(scope, ctx.author().id.get(), &links, 0);
    ctx.send(
        CreateReply::default()
            .content(content)
            .components(components)
            .ephemeral(true)
            .allowed_mentions(create_safe_allowed_mentions()),
    )
    .await?;
    Ok(())
}

/// 紐付け済みアカウントの一覧を表示します。
#[poise::command(
    slash_command,
    ephemeral,
    guild_only,
    subcommands("links_discord", "links_minecraft", "links_all")
)]
pub async fn links(_: AppApplicationContext<'_>) -> Result<(), AppError> {
    Ok(())
}

/// 指定した Discordアカウントの紐付け一覧を表示します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "discord")]
pub async fn links_discord(
    ctx: AppApplicationContext<'_>,
    #[description = "一覧表示する Discordユーザー"] user: User,
) -> Result<(), AppError> {
    send_command_page(ctx, Scope::Discord(user.id.get())).await
}

/// 指定した Minecraft UUID の紐付け一覧を表示します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "minecraft")]
pub async fn links_minecraft(
    ctx: AppApplicationContext<'_>,
    #[description = "一覧表示する Minecraft UUID"] uuid: String,
) -> Result<(), AppError> {
    if !moderator(ctx).await? {
        return Err(BotError::HasNoRole.into());
    }
    let Ok(uuid) = Uuid::parse_str(&uuid) else {
        ctx.say("Minecraft UUID は `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` 形式で指定してください。")
            .await?;
        return Ok(());
    };
    send_command_page(ctx, Scope::Minecraft(uuid)).await
}

/// 全ての紐付け一覧を表示します。
#[poise::command(slash_command, ephemeral, guild_only, rename = "all")]
pub async fn links_all(ctx: AppApplicationContext<'_>) -> Result<(), AppError> {
    send_command_page(ctx, Scope::All).await
}

fn parse_page(id: &str) -> Option<(Scope, u64, usize)> {
    let parts: Vec<_> = id.split(':').collect();
    match parts.as_slice() {
        ["ml", "u", owner, page] => {
            let owner = owner.parse().ok()?;
            Some((Scope::User(owner), owner, page.parse().ok()?))
        }
        ["ml", "d", owner, target, page] => Some((
            Scope::Discord(target.parse().ok()?),
            owner.parse().ok()?,
            page.parse().ok()?,
        )),
        ["ml", "m", owner, uuid, page] => Some((
            Scope::Minecraft(Uuid::parse_str(uuid).ok()?),
            owner.parse().ok()?,
            page.parse().ok()?,
        )),
        ["ml", "a", owner, page] => Some((Scope::All, owner.parse().ok()?, page.parse().ok()?)),
        _ => None,
    }
}

fn parse_unlink(id: &str, kind: &str) -> Option<(u64, Uuid)> {
    let parts: Vec<_> = id.split(':').collect();
    match parts.as_slice() {
        ["ml", action, owner, uuid] if *action == kind => Some((owner.parse().ok()?, Uuid::parse_str(uuid).ok()?)),
        _ => None,
    }
}

#[event_handler]
pub async fn handle_management_event(ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
    let config = ctx.app_config().await;
    let guild_id = config.mcguildlink.guild_id;
    match event {
        FullEvent::GuildMemberRemoval {
            guild_id: event_guild,
            user,
            ..
        } if *event_guild == guild_id => {
            LinkManagement::new(ctx.bot_data().database.clone())
                .member_left(user.id.get(), &user.name)
                .await?;
        }
        FullEvent::InteractionCreate {
            interaction: Interaction::Component(component),
            ..
        } if component.guild_id == Some(guild_id)
            && matches!(component.data.kind, ComponentInteractionDataKind::Button) =>
        {
            let id = component.data.custom_id.as_str();
            let store = LinkManagement::new(ctx.bot_data().database.clone());
            if id == LIST_LINK_BUTTON_ID {
                component.defer_ephemeral(&ctx.http).await?;
                let scope = Scope::User(component.user.id.get());
                let links = load(&store, scope).await?;
                let (content, components) = page(scope, component.user.id.get(), &links, 0);
                component
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content(content)
                            .components(components)
                            .allowed_mentions(create_safe_allowed_mentions()),
                    )
                    .await?;
            } else if let Some((scope, owner, page_index)) = parse_page(id) {
                if owner != component.user.id.get() {
                    component
                        .create_response(
                            &ctx.http,
                            create_ephemeral_message("不正な操作です。このボタンはあなたのものではありません。", None),
                        )
                        .await?;
                    return Ok(());
                }
                if scope.is_admin()
                    && !component
                        .member
                        .as_ref()
                        .is_some_and(|member| member.roles.contains(&config.mcguildlink.moderator_role_id))
                {
                    component
                        .create_response(&ctx.http, create_ephemeral_message("管理者権限が必要です。", None))
                        .await?;
                    return Ok(());
                }
                component.defer_ephemeral(&ctx.http).await?;
                let links = load(&store, scope).await?;
                let (content, components) = page(scope, owner, &links, page_index);
                component
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content(content)
                            .components(components)
                            .allowed_mentions(create_safe_allowed_mentions()),
                    )
                    .await?;
            } else if let Some((owner, uuid)) = parse_unlink(id, "x") {
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
                            CreateModal::new(format!("ml:c:{owner}:{uuid}"), "アカウントの紐付け解除").components(
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
            }
        }
        FullEvent::InteractionCreate {
            interaction: Interaction::Modal(modal),
            ..
        } if modal.guild_id == Some(guild_id) => {
            if let Some((owner, uuid)) = parse_unlink(&modal.data.custom_id, "c") {
                if owner != modal.user.id.get() {
                    modal
                        .create_response(
                            &ctx.http,
                            create_ephemeral_message(
                                "不正な操作です。このモーダルはあなたのものではありません。",
                                None,
                            ),
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
                let removed = LinkManagement::new(ctx.bot_data().database.clone())
                    .unlink(owner, uuid)
                    .await?;
                let content = if removed {
                    "アカウントの紐付けを解除しました。"
                } else {
                    "アカウント情報を取得できませんでした。すでに解除されている可能性があります。"
                };
                modal
                    .create_response(&ctx.http, create_ephemeral_message(content, None))
                    .await?;
            }
        }
        _ => {}
    }
    Ok(())
}
