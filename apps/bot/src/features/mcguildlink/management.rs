use super::{
    discord::LIST_LINK_BUTTON_ID,
    management_store::{Link, LinkManagement},
};
use crate::{
    app::{AppApplicationContext, AppError, BotDataExt, BotError},
    core::BotEventHandler,
    utils::{create_ephemeral_message, create_safe_allowed_mentions},
};
use dashmap::DashMap;
use poise::CreateReply;
use serenity::{
    all::{
        ButtonStyle, ComponentInteraction, ComponentInteractionDataKind, Context, CreateActionRow, CreateButton,
        EditInteractionResponse, FullEvent, GuildId, Interaction, LabelComponent, ModalComponent, ModalInteraction,
        RoleId, User,
    },
    async_trait,
    builder::{
        CreateCheckbox, CreateComponent, CreateInteractionResponse, CreateInteractionResponseMessage, CreateLabel,
        CreateModal, CreateModalComponent, CreateTextDisplay,
    },
};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;

const PAGE_SIZE: usize = 5;
const PAGE_BUTTON_PREFIX: &str = "account_links_page_button:";
const UNLINK_BUTTON_PREFIX: &str = "unlink_button:";
const UNLINK_MODAL_PREFIX: &str = "unlink_confirm_modal:";
const CONFIRM_ID: &str = "unlink_confirm_checkbox";
const SNAPSHOT_TTL: Duration = Duration::from_secs(600);

struct Snapshot {
    owner: u64,
    scope: Scope,
    links: Vec<Link>,
    expires_at: Instant,
}

static SNAPSHOTS: OnceLock<DashMap<u64, Arc<Snapshot>>> = OnceLock::new();

fn snapshots() -> &'static DashMap<u64, Arc<Snapshot>> {
    SNAPSHOTS.get_or_init(DashMap::new)
}

fn prune_snapshots() {
    let now = Instant::now();
    snapshots().retain(|_, snapshot| snapshot.expires_at > now);
}

fn save_snapshot(id: u64, owner: u64, scope: Scope, links: Vec<Link>) -> Arc<Snapshot> {
    prune_snapshots();
    let snapshot = Arc::new(Snapshot {
        owner,
        scope,
        links,
        expires_at: Instant::now() + SNAPSHOT_TTL,
    });
    snapshots().insert(id, snapshot.clone());
    snapshot
}

fn get_snapshot(id: u64) -> Option<Arc<Snapshot>> {
    prune_snapshots();
    snapshots().get(&id).map(|snapshot| snapshot.clone())
}

#[derive(Clone, Copy)]
enum Scope {
    User(u64),
    Discord(u64),
    Minecraft(Uuid),
    All,
}

impl Scope {
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

fn page(snapshot_id: u64, snapshot: &Snapshot, requested: usize) -> (String, Vec<CreateComponent<'static>>) {
    let scope = snapshot.scope;
    let owner = snapshot.owner;
    let links = &snapshot.links;
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
                CreateButton::new(format!("{UNLINK_BUTTON_PREFIX}{owner}:{}", link.minecraft_uuid))
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
            CreateButton::new(format!("{PAGE_BUTTON_PREFIX}{snapshot_id}:{}", index.saturating_sub(1)))
                .label("前へ")
                .style(ButtonStyle::Secondary)
                .disabled(index == 0),
            CreateButton::new(format!("{PAGE_BUTTON_PREFIX}{snapshot_id}:{}", index + 1))
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
    let data = ctx.bot_data();
    let store = LinkManagement::new(data.database.clone());
    let links = load(&store, scope).await?;
    let snapshot = save_snapshot(ctx.interaction.id.get(), ctx.author().id.get(), scope, links);
    let (content, components) = page(ctx.interaction.id.get(), &snapshot, 0);
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

fn parse_page(id: &str) -> Option<(u64, usize)> {
    let (snapshot_id, page) = id.strip_prefix(PAGE_BUTTON_PREFIX)?.split_once(':')?;
    Some((snapshot_id.parse().ok()?, page.parse().ok()?))
}

fn parse_unlink(id: &str, prefix: &str) -> Option<(u64, Uuid)> {
    let (owner, uuid) = id.strip_prefix(prefix)?.split_once(':')?;
    Some((owner.parse().ok()?, Uuid::parse_str(uuid).ok()?))
}

impl LinkManagementEventHandler {
    pub fn new(store: LinkManagement) -> Self {
        Self { store }
    }

    async fn handle_member_leave(
        &self,
        configured_guild: GuildId,
        event_guild: GuildId,
        user_id: u64,
        username: &str,
    ) -> Result<(), AppError> {
        if event_guild == configured_guild {
            self.store.member_left(user_id, username).await?;
        }
        Ok(())
    }

    async fn handle_list_button(&self, ctx: &Context, component: &ComponentInteraction) -> Result<(), AppError> {
        component.defer_ephemeral(&ctx.http).await?;
        let scope = Scope::User(component.user.id.get());
        let links = load(&self.store, scope).await?;
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

    async fn handle_page_button(
        &self,
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

    async fn handle_unlink_button(
        &self,
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
        let link = self
            .store
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
                    CreateModal::new(format!("{UNLINK_MODAL_PREFIX}{owner}:{uuid}"), "アカウントの紐付け解除")
                        .components(vec![
                            CreateModalComponent::TextDisplay(CreateTextDisplay::new(format!(
                                "本当に **{}** との紐付けを解除しますか？ この操作は取り消せません。",
                                link.minecraft_name,
                            ))),
                            CreateModalComponent::Label(CreateLabel::checkbox(
                                "内容を確認し、解除に同意します。",
                                CreateCheckbox::new(CONFIRM_ID).default_selected(false),
                            )),
                        ]),
                ),
            )
            .await?;

        Ok(())
    }

    async fn handle_component(
        &self,
        ctx: &Context,
        component: &ComponentInteraction,
        moderator_role: RoleId,
    ) -> Result<(), AppError> {
        let id = component.data.custom_id.as_str();
        if id == LIST_LINK_BUTTON_ID {
            self.handle_list_button(ctx, component).await?;
        } else if let Some((snapshot_id, page_index)) = parse_page(id) {
            self.handle_page_button(ctx, component, moderator_role, snapshot_id, page_index)
                .await?;
        } else if let Some((owner, uuid)) = parse_unlink(id, UNLINK_BUTTON_PREFIX) {
            self.handle_unlink_button(ctx, component, owner, uuid).await?;
        }
        Ok(())
    }

    async fn handle_unlink_modal(&self, ctx: &Context, modal: &ModalInteraction) -> Result<(), AppError> {
        if let Some((owner, uuid)) = parse_unlink(&modal.data.custom_id, UNLINK_MODAL_PREFIX) {
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
            let removed = self.store.unlink(owner, uuid).await?;
            let content = if removed {
                "アカウントの紐付けを解除しました。"
            } else {
                "アカウント情報を取得できませんでした。すでに解除されている可能性があります。"
            };
            modal
                .create_response(&ctx.http, create_ephemeral_message(content, None))
                .await?;
        }
        Ok(())
    }
}

pub struct LinkManagementEventHandler {
    store: LinkManagement,
}

#[async_trait]
impl BotEventHandler for LinkManagementEventHandler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) -> Result<(), AppError> {
        let config = ctx.app_config().await;
        let guild_id = config.mcguildlink.guild_id;
        match event {
            FullEvent::GuildMemberRemoval {
                guild_id: event_guild,
                user,
                ..
            } => {
                self.handle_member_leave(guild_id, *event_guild, user.id.get(), &user.name)
                    .await?;
            }
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(component),
                ..
            } if component.guild_id == Some(guild_id)
                && matches!(component.data.kind, ComponentInteractionDataKind::Button) =>
            {
                self.handle_component(ctx, component, config.mcguildlink.moderator_role_id)
                    .await?;
            }
            FullEvent::InteractionCreate {
                interaction: Interaction::Modal(modal),
                ..
            } if modal.guild_id == Some(guild_id) => {
                self.handle_unlink_modal(ctx, modal).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::mcguildlink::test_support;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "../../migrations")]
    async fn other_guild_leave_keeps_links_code_and_audit_unchanged(pool: PgPool) {
        sqlx::raw_sql(
            "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice');
            INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name)
                VALUES ('00000000-0000-0000-0000-000000000001', 'First');
            INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 1);
            INSERT INTO mcguildlink.link_requests (discord_account_id, code) VALUES (1, 'ALICECODE');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let store = LinkManagement::new(test_support::bot_pool(&pool).await);
        LinkManagementEventHandler::new(store.clone())
            .handle_member_leave(GuildId::new(100), GuildId::new(200), 10, "alice")
            .await
            .unwrap();
        assert_eq!(store.by_discord(10).await.unwrap().len(), 1);
        let codes: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.link_requests")
            .fetch_one(&pool)
            .await
            .unwrap();
        let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM mcguildlink.audit_logs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((codes, audit), (1, 0));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn page_uses_initial_snapshot_after_a_link_changes(pool: PgPool) {
        sqlx::raw_sql(
            "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES (10, 'alice');
            INSERT INTO mcguildlink.minecraft_accounts (uuid, last_known_name)
                VALUES ('00000000-0000-0000-0000-000000000001', 'First');
            INSERT INTO mcguildlink.account_links (discord_account_id, minecraft_account_id) VALUES (1, 1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let store = LinkManagement::new(test_support::bot_pool(&pool).await);
        save_snapshot(1000, 10, Scope::User(10), store.by_discord(10).await.unwrap());
        assert!(
            store
                .unlink(10, Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
                .await
                .unwrap()
        );
        assert!(store.by_discord(10).await.unwrap().is_empty());
        let snapshot = get_snapshot(1000).unwrap();
        assert!(page(1000, &snapshot, 0).0.contains("First"));
        assert_eq!(snapshot.links.len(), 1);
    }
}
