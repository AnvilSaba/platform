use super::repository::BlockGroup;
use dashmap::DashMap;
use serenity::{
    all::{ButtonStyle, CreateActionRow, CreateButton},
    builder::{CreateComponent, CreateContainer, CreateContainerComponent, CreateTextDisplay},
};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

const PREFIX: &str = "blocked_accounts_page:";
const PAGE_SIZE: usize = 10;
const TTL: Duration = Duration::from_secs(600);

pub struct Snapshot {
    pub owner: u64,
    groups: Vec<BlockGroup>,
    expires_at: Instant,
}

static SNAPSHOTS: OnceLock<DashMap<u64, Arc<Snapshot>>> = OnceLock::new();
fn snapshots() -> &'static DashMap<u64, Arc<Snapshot>> {
    SNAPSHOTS.get_or_init(DashMap::new)
}

pub fn save(id: u64, owner: u64, groups: Vec<BlockGroup>) -> Arc<Snapshot> {
    snapshots().retain(|_, snapshot| snapshot.expires_at > Instant::now());
    let snapshot = Arc::new(Snapshot {
        owner,
        groups,
        expires_at: Instant::now() + TTL,
    });
    snapshots().insert(id, snapshot.clone());
    snapshot
}

pub fn get(id: u64) -> Option<Arc<Snapshot>> {
    snapshots().retain(|_, snapshot| snapshot.expires_at > Instant::now());
    snapshots().get(&id).map(|item| item.clone())
}

pub fn parse(id: &str) -> Option<(u64, usize)> {
    let (snapshot, page) = id.strip_prefix(PREFIX)?.split_once(':')?;
    Some((snapshot.parse().ok()?, page.parse().ok()?))
}

pub fn render(id: u64, snapshot: &Snapshot, requested: usize) -> Vec<CreateComponent<'static>> {
    let pages = snapshot.groups.len().div_ceil(PAGE_SIZE);
    let page = requested.min(pages.saturating_sub(1));
    let mut components = vec![CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
        "## ブロックされているアカウント\n`/block remove` で所属するグループ全体を解除できます。",
    ))];
    for group in snapshot.groups.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
        let mut text = String::from("- ブロック中の Discordアカウント\n");
        for account in &group.discord {
            text.push_str(&format!(
                "  - {} (`{}`){}\n",
                account.name,
                account.user_id,
                if account.user_id == group.root.user_id {
                    " (root)"
                } else {
                    ""
                }
            ));
        }
        if !group.minecraft.is_empty() {
            text.push_str("- ブロック中の Minecraftアカウント\n");
            for account in &group.minecraft {
                text.push_str(&format!("  - {} (`{}`)\n", account.name, account.uuid));
            }
        }
        text.push_str(&format!("- 作成日時: <t:{}:F>", group.created_at.timestamp()));
        components.push(CreateContainerComponent::TextDisplay(CreateTextDisplay::new(text)));
    }
    if pages > 1 {
        components.push(CreateContainerComponent::ActionRow(CreateActionRow::buttons(vec![
            CreateButton::new(format!("{PREFIX}{id}:{}", page.saturating_sub(1)))
                .label("◀")
                .style(ButtonStyle::Primary)
                .disabled(page == 0),
            CreateButton::new(format!("{PREFIX}disabled"))
                .label(format!("{} / {} ({})", page + 1, pages, snapshot.groups.len()))
                .style(ButtonStyle::Secondary)
                .disabled(true),
            CreateButton::new(format!("{PREFIX}{id}:{}", (page + 1).min(pages - 1)))
                .label("▶")
                .style(ButtonStyle::Primary)
                .disabled(page + 1 >= pages),
        ])));
    }
    vec![CreateComponent::Container(CreateContainer::new(components))]
}
