use super::store::{Link, LinkManagement};
use crate::app::AppError;
use dashmap::DashMap;
use serenity::{
    all::{ButtonStyle, CreateActionRow, CreateButton},
    builder::CreateComponent,
};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;

const PAGE_SIZE: usize = 5;
const PAGE_BUTTON_PREFIX: &str = "account_links_page_button:";
const UNLINK_BUTTON_PREFIX: &str = "unlink_button:";
const SNAPSHOT_TTL: Duration = Duration::from_secs(600);

pub(super) struct Snapshot {
    pub(super) owner: u64,
    pub(super) scope: Scope,
    pub(super) links: Vec<Link>,
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

pub(super) fn save_snapshot(id: u64, owner: u64, scope: Scope, links: Vec<Link>) -> Arc<Snapshot> {
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

pub(super) fn get_snapshot(id: u64) -> Option<Arc<Snapshot>> {
    prune_snapshots();
    snapshots().get(&id).map(|snapshot| snapshot.clone())
}

#[derive(Clone, Copy)]
pub(super) enum Scope {
    User(u64),
    Discord(u64),
    Minecraft(Uuid),
    All,
}

impl Scope {
    pub(super) fn is_admin(self) -> bool {
        !matches!(self, Self::User(_))
    }
}

pub(super) async fn load(store: &LinkManagement, scope: Scope) -> Result<Vec<Link>, AppError> {
    match scope {
        Scope::User(id) | Scope::Discord(id) => store.by_discord(id).await,
        Scope::Minecraft(uuid) => store.by_minecraft(uuid).await,
        Scope::All => store.all().await,
    }
}

pub(super) fn page(snapshot_id: u64, snapshot: &Snapshot, requested: usize) -> (String, Vec<CreateComponent<'static>>) {
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

pub(super) fn parse_page(id: &str) -> Option<(u64, usize)> {
    let (snapshot_id, page) = id.strip_prefix(PAGE_BUTTON_PREFIX)?.split_once(':')?;
    Some((snapshot_id.parse().ok()?, page.parse().ok()?))
}
