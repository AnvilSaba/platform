use std::sync::{Arc, OnceLock};

use serenity::builder::{CreateComponent, CreateContainer, CreateContainerComponent, CreateTextDisplay};

use super::repository::BlockGroup;
use crate::features::mcguildlink::pagination::{Pagination, Snapshot as PageSnapshot};

const PREFIX: &str = "blocked_accounts_page:";
const PAGE_SIZE: usize = 10;

pub(super) type Snapshot = PageSnapshot<BlockGroup, ()>;
static SNAPSHOTS: OnceLock<Pagination<BlockGroup, ()>> = OnceLock::new();

fn pagination() -> &'static Pagination<BlockGroup, ()> {
    SNAPSHOTS.get_or_init(|| Pagination::new(PREFIX))
}

pub fn save(id: u64, owner: u64, groups: Vec<BlockGroup>) -> Arc<Snapshot> {
    pagination().save(id, owner, (), groups)
}

pub fn get(id: u64) -> Option<Arc<Snapshot>> {
    pagination().get(id)
}

pub fn parse(id: &str) -> Option<(u64, usize)> {
    pagination().parse(id)
}

pub fn render(id: u64, snapshot: &Snapshot, requested: usize) -> Vec<CreateComponent<'static>> {
    let visible = snapshot.page(requested, PAGE_SIZE);
    let mut components = vec![CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
        "## ブロックされているアカウント\n`/block remove` で所属するグループ全体を解除できます。",
    ))];
    for group in visible.entries {
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
    if let Some(controls) = pagination().controls(id, visible.index, visible.pages, visible.total) {
        components.push(controls);
    }
    vec![CreateComponent::Container(CreateContainer::new(components))]
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::super::repository::DiscordAccount;
    use super::*;

    #[test]
    fn list_pages_keep_the_first_ten_groups_and_root_labels() {
        let groups: Vec<_> = (1..=11)
            .map(|id| {
                let root = DiscordAccount {
                    user_id: id.to_string(),
                    name: format!("User{id}"),
                };
                BlockGroup {
                    root: root.clone(),
                    discord: vec![root],
                    minecraft: vec![],
                    created_at: Utc::now(),
                }
            })
            .collect();
        let snapshot = Snapshot::new(42, (), groups);
        let first = serde_json::to_string(&render(99, &snapshot, 0)).unwrap();
        let second = serde_json::to_string(&render(99, &snapshot, 1)).unwrap();
        assert!(first.contains("User1"));
        assert!(first.contains("User10"));
        assert!(!first.contains("User11"));
        assert!(first.contains("(root)"));
        assert!(first.contains("1 / 2 (11)"));
        assert!(second.contains("User11"));
        assert!(!second.contains("User10"));
        assert!(second.contains("2 / 2 (11)"));
    }
}
