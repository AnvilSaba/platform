use dashmap::DashMap;
use serenity::{
    all::{ButtonStyle, CreateActionRow, CreateButton},
    builder::CreateContainerComponent,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const SNAPSHOT_TTL: Duration = Duration::from_secs(600);

pub(super) struct Snapshot<T, C> {
    pub owner: u64,
    pub context: C,
    pub entries: Vec<T>,
    expires_at: Instant,
}

impl<T, C> Snapshot<T, C> {
    pub fn new(owner: u64, context: C, entries: Vec<T>) -> Self {
        Self {
            owner,
            context,
            entries,
            expires_at: Instant::now() + SNAPSHOT_TTL,
        }
    }

    pub fn page(&self, requested: usize, page_size: usize) -> Page<'_, T> {
        let pages = self.entries.len().div_ceil(page_size);
        let index = requested.min(pages.saturating_sub(1));
        Page {
            index,
            pages,
            total: self.entries.len(),
            entries: &self.entries[index * page_size..self.entries.len().min((index + 1) * page_size)],
        }
    }
}

pub(super) struct Page<'a, T> {
    pub index: usize,
    pub pages: usize,
    pub total: usize,
    pub entries: &'a [T],
}

pub(super) struct Pagination<T, C> {
    prefix: &'static str,
    snapshots: DashMap<u64, Arc<Snapshot<T, C>>>,
}

impl<T, C> Pagination<T, C> {
    pub fn new(prefix: &'static str) -> Self {
        Self {
            prefix,
            snapshots: DashMap::new(),
        }
    }

    pub fn save(&self, id: u64, owner: u64, context: C, entries: Vec<T>) -> Arc<Snapshot<T, C>> {
        self.prune();
        let snapshot = Arc::new(Snapshot::new(owner, context, entries));
        self.snapshots.insert(id, snapshot.clone());
        snapshot
    }

    pub fn get(&self, id: u64) -> Option<Arc<Snapshot<T, C>>> {
        self.prune();
        self.snapshots.get(&id).map(|item| item.clone())
    }

    pub fn parse(&self, id: &str) -> Option<(u64, usize)> {
        let (snapshot, page) = id.strip_prefix(self.prefix)?.split_once(':')?;
        Some((snapshot.parse().ok()?, page.parse().ok()?))
    }

    pub fn controls(
        &self,
        id: u64,
        index: usize,
        pages: usize,
        total: usize,
    ) -> Option<CreateContainerComponent<'static>> {
        if pages <= 1 {
            return None;
        }
        let button = |page| format!("{}{id}:{page}", self.prefix);
        Some(CreateContainerComponent::ActionRow(CreateActionRow::buttons(vec![
            CreateButton::new(button(index.saturating_sub(1)))
                .label("◀")
                .style(ButtonStyle::Primary)
                .disabled(index == 0),
            CreateButton::new(format!("{}disabled", self.prefix))
                .label(format!("{} / {} ({total})", index + 1, pages))
                .style(ButtonStyle::Secondary)
                .disabled(true),
            CreateButton::new(button((index + 1).min(pages - 1)))
                .label("▶")
                .style(ButtonStyle::Primary)
                .disabled(index + 1 >= pages),
        ])))
    }

    fn prune(&self) {
        let now = Instant::now();
        self.snapshots.retain(|_, snapshot| snapshot.expires_at > now);
    }
}
