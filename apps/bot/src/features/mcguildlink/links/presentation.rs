use super::{model::Link, ports::AccountLinksRepository};
use crate::app::AppError;
use crate::features::mcguildlink::pagination::{Pagination, Snapshot as PageSnapshot};
use serenity::{
    all::{ButtonStyle, CreateActionRow, CreateButton, ReactionType, SeparatorSpacingSize},
    builder::{
        CreateComponent, CreateContainer, CreateContainerComponent, CreateSection, CreateSectionAccessory,
        CreateSectionComponent, CreateSeparator, CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem,
    },
};
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

const PAGE_SIZE: usize = 5;
const PAGE_BUTTON_PREFIX: &str = "account_links_page_button:";
const UNLINK_BUTTON_PREFIX: &str = "unlink_button:";
pub(super) type Snapshot = PageSnapshot<Link, Scope>;

static SNAPSHOTS: OnceLock<Pagination<Link, Scope>> = OnceLock::new();

fn pagination() -> &'static Pagination<Link, Scope> {
    SNAPSHOTS.get_or_init(|| Pagination::new(PAGE_BUTTON_PREFIX))
}

pub(super) fn save_snapshot(id: u64, owner: u64, scope: Scope, links: Vec<Link>) -> Arc<Snapshot> {
    pagination().save(id, owner, scope, links)
}

pub(super) fn get_snapshot(id: u64) -> Option<Arc<Snapshot>> {
    pagination().get(id)
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

pub(super) async fn load(repository: &impl AccountLinksRepository, scope: Scope) -> Result<Vec<Link>, AppError> {
    match scope {
        Scope::User(id) | Scope::Discord(id) => repository.by_discord(id).await,
        Scope::Minecraft(uuid) => repository.by_minecraft(uuid).await,
        Scope::All => repository.all().await,
    }
}

pub(super) enum ListPage {
    Empty(&'static str),
    Components(Vec<CreateComponent<'static>>),
}

fn header_separator() -> CreateContainerComponent<'static> {
    CreateContainerComponent::Separator(CreateSeparator::new().divider(false))
}

fn large_divider() -> CreateContainerComponent<'static> {
    CreateContainerComponent::Separator(CreateSeparator::new().spacing(SeparatorSpacingSize::Large))
}

fn account_section(text: String, uuid: Uuid) -> CreateContainerComponent<'static> {
    let thumbnail = CreateThumbnail::new(CreateUnfurledMediaItem::new(format!(
        "https://mc-heads.net/avatar/{uuid}"
    )));
    CreateContainerComponent::Section(CreateSection::new(
        vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(text))],
        CreateSectionAccessory::Thumbnail(thumbnail),
    ))
}

fn append_user_link(components: &mut Vec<CreateContainerComponent<'static>>, owner: u64, link: &Link) {
    components.push(large_divider());
    components.push(account_section(
        format!("- 名前: **{}**\n- UUID: `{}`", link.minecraft_name, link.minecraft_uuid),
        link.minecraft_uuid,
    ));
    components.push(CreateContainerComponent::ActionRow(CreateActionRow::buttons(vec![
        CreateButton::new(format!("{UNLINK_BUTTON_PREFIX}{owner}:{}", link.minecraft_uuid))
            .label("解除")
            .emoji(ReactionType::Unicode("🗑️".to_owned().try_into().expect("valid emoji")))
            .style(ButtonStyle::Danger),
    ])));
}

fn append_admin_link(components: &mut Vec<CreateContainerComponent<'static>>, link: &Link, first: bool) {
    if !first {
        components.push(large_divider());
    }
    components.push(account_section(
        format!(
            "- Discord: {} (`{}`)\n- Minecraft: {} (`{}`)\n- 紐付け日時: <t:{}:F>",
            link.discord_name,
            link.discord_user_id,
            link.minecraft_name,
            link.minecraft_uuid,
            link.linked_at.timestamp(),
        ),
        link.minecraft_uuid,
    ));
}

pub(super) fn page(snapshot_id: u64, snapshot: &Snapshot, requested: usize) -> ListPage {
    let scope = snapshot.context;
    let owner = snapshot.owner;
    if snapshot.entries.is_empty() {
        let empty = match scope {
            Scope::User(_) => "あなたの Discordアカウントに紐付けられた Minecraftアカウントはありません。",
            Scope::Discord(_) => "その Discordアカウントに紐付けられている Minecraftアカウントはありません。",
            Scope::Minecraft(_) => "その Minecraftアカウントに紐付けられている Discordアカウントはありません。",
            Scope::All => "紐付け済みアカウントはありません。",
        };
        return ListPage::Empty(empty);
    }

    let visible = snapshot.page(requested, PAGE_SIZE);
    let heading = match scope {
        Scope::User(_) => {
            "## 紐付けられたアカウント\n以下の Minecraftアカウントがあなたの Discordアカウントに紐付けられています。\n紐付けを解除したいアカウントがある場合は、各アカウントの「解除」ボタンを押してください。"
        }
        Scope::Discord(_) => {
            "## 紐付け一覧 (Discord)\n指定した Discordアカウントに紐付けられている Minecraftアカウントを表示しています。"
        }
        Scope::Minecraft(_) => {
            "## 紐付け一覧 (Minecraft)\n指定した Minecraftアカウントに紐付けられている Discordアカウントを表示しています。"
        }
        Scope::All => "## 紐付け一覧 (All)\n現在の全ての紐付けを新しい順に表示しています。",
    };
    let mut components = vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(heading)),
        header_separator(),
    ];
    for (item, link) in visible.entries.iter().enumerate() {
        match scope {
            Scope::User(_) => append_user_link(&mut components, owner, link),
            _ => append_admin_link(&mut components, link, item == 0),
        }
    }
    if let Some(controls) = pagination().controls(snapshot_id, visible.index, visible.pages, visible.total) {
        components.push(large_divider());
        components.push(controls);
    }
    ListPage::Components(vec![CreateComponent::Container(CreateContainer::new(components))])
}

pub(super) fn parse_page(id: &str) -> Option<(u64, usize)> {
    pagination().parse(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn link(index: u128) -> Link {
        Link {
            discord_user_id: "123".into(),
            discord_name: "DiscordUser".into(),
            minecraft_uuid: Uuid::from_u128(index),
            minecraft_name: format!("Minecraft{index}"),
            linked_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        }
    }

    fn rendered(scope: Scope, links: Vec<Link>, requested: usize) -> String {
        let snapshot = Snapshot::new(123, scope, links);
        let ListPage::Components(components) = page(456, &snapshot, requested) else {
            panic!("expected a components V2 list");
        };
        serde_json::to_string(&components).unwrap()
    }

    #[test]
    fn user_list_preserves_legacy_account_card_and_pagination() {
        let json = rendered(Scope::User(123), (1..=6).map(link).collect(), 0);
        assert!(
            json.contains("紐付けを解除したいアカウントがある場合は、各アカウントの「解除」ボタンを押してください。")
        );
        assert!(json.contains("- 名前: **Minecraft1**\\n- UUID: `00000000-0000-0000-0000-000000000001`"));
        assert!(json.contains("https://mc-heads.net/avatar/00000000-0000-0000-0000-000000000001"));
        assert!(json.contains("🗑️"));
        assert!(json.contains("1 / 2 (6)"));
        assert!(json.contains("◀"));
        assert!(json.contains("▶"));
        assert_eq!(json.matches("\"divider\":false").count(), 1);
        assert_eq!(json.matches("\"spacing\":2").count(), 6);
        assert_eq!(json.matches("unlink_button:").count(), 5);
        assert!(!json.contains("Minecraft6"));
    }

    #[test]
    fn admin_list_preserves_legacy_section_text_without_unlink_buttons() {
        let json = rendered(Scope::Discord(123), vec![link(1)], 0);
        assert!(json.contains("指定した Discordアカウントに紐付けられている Minecraftアカウントを表示しています。"));
        assert!(json.contains("- Discord: DiscordUser (`123`)\\n- Minecraft: Minecraft1 (`00000000-0000-0000-0000-000000000001`)\\n- 紐付け日時: <t:1700000000:F>"));
        assert!(!json.contains("unlink_button:"));
        assert!(!json.contains("1 / 1"));
    }

    #[test]
    fn empty_list_uses_legacy_plain_message() {
        let snapshot = Snapshot::new(123, Scope::User(123), vec![]);
        assert!(matches!(
            page(456, &snapshot, 0),
            ListPage::Empty("あなたの Discordアカウントに紐付けられた Minecraftアカウントはありません。")
        ));
    }
}
