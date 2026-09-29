use std::{sync::Arc, time::Duration};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serenity::{
    all::{ChannelId, CreateEmbed, CreateMessage, Http},
    async_trait,
};
use sqlx::{PgPool, types::Json};
use tracing::warn;
use uuid::Uuid;

use crate::{app::BotData, utils::create_safe_message};

const BATCH_SIZE: i64 = 100;
const MAX_DESCRIPTION_UNITS: usize = 4_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditPost {
    pub channel_id: ChannelId,
    pub event_id: i64,
    pub occurred_at: DateTime<Utc>,
    pub title: String,
    pub description: String,
    pub color: u32,
}

impl AuditPost {
    fn into_message(self) -> CreateMessage<'static> {
        let description = if self.description.encode_utf16().count() > MAX_DESCRIPTION_UNITS {
            let mut shortened = String::new();
            let mut units = 0;
            for ch in self.description.chars() {
                if units + ch.len_utf16() > MAX_DESCRIPTION_UNITS - 12 {
                    break;
                }
                shortened.push(ch);
                units += ch.len_utf16();
            }
            shortened.push_str("\n…（以下省略）");
            shortened
        } else {
            self.description
        };
        let embed = CreateEmbed::new()
            .title(self.title)
            .description(description)
            .color(self.color);
        create_safe_message()
            .content(format!(
                "発生日時: <t:{}:F> · イベントID: {}",
                self.occurred_at.timestamp(),
                self.event_id
            ))
            .embed(embed)
    }
}

#[async_trait]
pub trait AuditSender: Send + Sync {
    async fn send(&self, post: AuditPost) -> Result<()>;
}

pub struct DiscordAuditSender {
    http: Arc<Http>,
}

impl DiscordAuditSender {
    pub fn new(http: Arc<Http>) -> Self {
        Self { http }
    }
}

#[async_trait]
impl AuditSender for DiscordAuditSender {
    async fn send(&self, post: AuditPost) -> Result<()> {
        let channel_id = post.channel_id;
        channel_id.widen().send_message(&self.http, post.into_message()).await?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct DiscordSnapshot {
    user_id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftSnapshot {
    uuid: Uuid,
    name: String,
}

#[derive(sqlx::FromRow)]
struct PendingAudit {
    id: i64,
    event_type: String,
    occurred_at: DateTime<Utc>,
    actor_type: String,
    actor_minecraft_uuid: Option<Uuid>,
    actor_minecraft_name: Option<String>,
    actor_discord_user_id: Option<String>,
    actor_discord_username: Option<String>,
    target_discord_user_id: String,
    target_discord_username: String,
    target_minecraft_uuid: Option<Uuid>,
    target_minecraft_name: Option<String>,
    related_discord_accounts: Option<Json<Vec<DiscordSnapshot>>>,
    related_minecraft_accounts: Option<Json<Vec<MinecraftSnapshot>>>,
}

impl PendingAudit {
    fn post(self, channel_id: ChannelId) -> AuditPost {
        let target_discord = format!("{} ({})", self.target_discord_username, self.target_discord_user_id);
        let target_minecraft = self
            .target_minecraft_uuid
            .zip(self.target_minecraft_name)
            .map(|(uuid, name)| format!("{name} ({uuid})"));
        let (title, mut description) = match self.event_type.as_str() {
            "link_succeeded" => (
                "アカウントの紐付けが完了しました。".to_owned(),
                format!(
                    "Discordユーザー: {target_discord}\nMinecraftアカウント: {}",
                    target_minecraft.unwrap_or_default()
                ),
            ),
            "member_leave_unlinked" => (
                "メンバーの退出により紐付けを自動解除しました。".to_owned(),
                format!(
                    "Discordユーザー: {target_discord}\nMinecraftアカウント: {}",
                    target_minecraft.unwrap_or_default()
                ),
            ),
            "member_banned_blocked" => {
                let mut description = format!("BAN対象のDiscordユーザー: {target_discord}");
                if let Some(accounts) = self.related_discord_accounts {
                    description.push_str("\nブロックしたDiscordアカウント:");
                    for account in accounts.0 {
                        description.push_str(&format!("\n- {} ({})", account.name, account.user_id));
                    }
                }
                if let Some(accounts) = self.related_minecraft_accounts {
                    description.push_str("\nブロックしたMinecraftアカウント:");
                    for account in accounts.0 {
                        description.push_str(&format!("\n- {} ({})", account.name, account.uuid));
                    }
                }
                (
                    "メンバーのBANにより関連アカウントを自動ブロックしました。".to_owned(),
                    description,
                )
            }
            _ => (
                format!("監査イベント: {}", self.event_type),
                format!("Discordユーザー: {target_discord}"),
            ),
        };
        match self.actor_type.as_str() {
            "minecraft_player" => {
                if let Some((uuid, name)) = self.actor_minecraft_uuid.zip(self.actor_minecraft_name) {
                    description.push_str(&format!("\n操作主体: Minecraft {name} ({uuid})"));
                }
            }
            "discord_member" => {
                if let Some((id, name)) = self.actor_discord_user_id.zip(self.actor_discord_username) {
                    description.push_str(&format!("\n操作主体: Discord {name} ({id})"));
                }
            }
            "system" => description.push_str("\n操作主体: システム"),
            _ => description.push_str(&format!("\n操作主体: {}", self.actor_type)),
        }
        AuditPost {
            channel_id,
            event_id: self.id,
            occurred_at: self.occurred_at,
            title,
            description,
            color: 0x57F287,
        }
    }
}

pub struct AuditDelivery<S> {
    pool: PgPool,
    sender: S,
    channel_id: ChannelId,
}

impl<S: AuditSender> AuditDelivery<S> {
    pub fn new(pool: PgPool, sender: S, channel_id: ChannelId) -> Self {
        Self {
            pool,
            sender,
            channel_id,
        }
    }

    pub async fn deliver_pending(&self) -> Result<()> {
        let pending = sqlx::query_as::<_, PendingAudit>(
            "SELECT a.id, a.event_type, a.occurred_at, a.actor_type, a.actor_minecraft_uuid,
                    a.actor_minecraft_name, a.actor_discord_user_id::text AS actor_discord_user_id,
                    a.actor_discord_username, a.target_discord_user_id::text AS target_discord_user_id,
                    a.target_discord_username, a.target_minecraft_uuid, a.target_minecraft_name,
                    a.related_discord_accounts, a.related_minecraft_accounts
             FROM mcguildlink.audit_outbox o
             JOIN mcguildlink.audit_logs a ON a.id = o.log_id
             WHERE NOT o.needs_attention AND o.next_attempt_at <= now()
             ORDER BY o.log_id LIMIT $1",
        )
        .bind(BATCH_SIZE)
        .fetch_all(&self.pool)
        .await?;
        for event in pending {
            let id = event.id;
            if let Err(error) = self.sender.send(event.post(self.channel_id)).await {
                warn!(event_id = id, "監査ログの配送に失敗しました: {error:#}");
                continue;
            }
            sqlx::query("DELETE FROM mcguildlink.audit_outbox WHERE log_id = $1")
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
}

pub async fn run_delivery(data: Arc<BotData>, http: Arc<Http>) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let config = data.app_config().await;
        let delivery = AuditDelivery::new(
            data.database.clone(),
            DiscordAuditSender::new(http.clone()),
            config.mcguildlink.audit_channel_id,
        );
        if let Err(error) = delivery.deliver_pending().await {
            warn!("監査ログの読み取りまたは削除に失敗しました: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::mcguildlink::test_support;
    use std::{sync::Mutex, time::Duration};
    use tokio::sync::Notify;

    #[derive(Default)]
    struct CapturingSender(Mutex<Vec<AuditPost>>);

    #[async_trait]
    impl AuditSender for Arc<CapturingSender> {
        async fn send(&self, post: AuditPost) -> Result<()> {
            self.0.lock().unwrap().push(post);
            Ok(())
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn delivers_database_audit_and_keeps_history(pool: PgPool) {
        sqlx::query(
            "INSERT INTO mcguildlink.audit_logs
             (event_type, occurred_at, actor_type, actor_minecraft_uuid, actor_minecraft_name,
              target_discord_user_id, target_discord_username, target_minecraft_uuid, target_minecraft_name)
             VALUES ('link_succeeded', '2026-09-29 12:34:56+00', 'minecraft_player',
                     '00000000-0000-0000-0000-000000000001', 'AliceOld', 42, 'DiscordOld',
                     '00000000-0000-0000-0000-000000000001', 'AliceOld')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let sender = Arc::new(CapturingSender::default());
        let delivery = AuditDelivery::new(test_support::bot_pool(&pool).await, sender.clone(), ChannelId::new(123));
        delivery.deliver_pending().await.unwrap();

        let sent = sender.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].channel_id, ChannelId::new(123));
        assert!(sent[0].description.contains("DiscordOld"));
        assert!(sent[0].description.contains("AliceOld"));
        assert!(sent[0].description.contains("操作主体: Minecraft"));
        assert_eq!(sent[0].event_id, 1);
        assert_eq!(sent[0].occurred_at.timestamp(), 1_790_685_296);
        let message = serde_json::to_string(&sent[0].clone().into_message()).unwrap();
        assert!(message.contains("<t:1790685296:F>"));
        assert!(message.contains("イベントID: 1"));
        assert!(message.contains("DiscordOld"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcguildlink.audit_outbox")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcguildlink.audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    #[derive(Default)]
    struct NotifyingSender {
        sent: Mutex<Vec<AuditPost>>,
        sent_signal: Notify,
    }

    #[async_trait]
    impl AuditSender for Arc<NotifyingSender> {
        async fn send(&self, post: AuditPost) -> Result<()> {
            self.sent.lock().unwrap().push(post);
            self.sent_signal.notify_one();
            Ok(())
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn restart_after_post_before_delete_delivers_same_event_again(pool: PgPool) {
        sqlx::query(
            "INSERT INTO mcguildlink.audit_logs
             (event_type, actor_type, target_discord_user_id, target_discord_username,
              target_minecraft_uuid, target_minecraft_name)
             VALUES ('member_leave_unlinked', 'system', 42, 'left-user',
                     '00000000-0000-0000-0000-000000000001', 'OldName')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // 投稿完了後の DELETE だけを止め、Bot 停止時に outbox が残ることを確認する。
        let mut locked = pool.begin().await.unwrap();
        sqlx::query("SELECT log_id FROM mcguildlink.audit_outbox FOR UPDATE")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        let sender = Arc::new(NotifyingSender::default());
        let delivery = AuditDelivery::new(test_support::bot_pool(&pool).await, sender.clone(), ChannelId::new(123));
        let task = tokio::spawn(async move { delivery.deliver_pending().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(10), sender.sent_signal.notified())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!task.is_finished());
        task.abort();
        task.await.unwrap_err();
        locked.rollback().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcguildlink.audit_outbox")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );

        let retry_sender = Arc::new(CapturingSender::default());
        AuditDelivery::new(
            test_support::bot_pool(&pool).await,
            retry_sender.clone(),
            ChannelId::new(456),
        )
        .deliver_pending()
        .await
        .unwrap();
        let first = sender.sent.lock().unwrap();
        let second = retry_sender.0.lock().unwrap();
        assert_eq!(first[0].event_id, second[0].event_id);
        assert_eq!(second[0].channel_id, ChannelId::new(456));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcguildlink.audit_outbox")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn delivers_bot_written_leave_and_ban_snapshots(pool: PgPool) {
        let bot = test_support::bot_pool(&pool).await;
        sqlx::query(
            "INSERT INTO mcguildlink.audit_logs
             (event_type, actor_type, actor_discord_user_id, actor_discord_username,
              target_discord_user_id, target_discord_username,
              target_minecraft_uuid, target_minecraft_name)
             VALUES ('member_leave_unlinked', 'discord_member', 55, 'BeforeLeave',
                     55, 'BeforeLeave', '00000000-0000-0000-0000-000000000002', 'OldPlayer')",
        )
        .execute(&bot)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO mcguildlink.audit_logs
             (event_type, actor_type, target_discord_user_id, target_discord_username,
              related_discord_accounts, related_minecraft_accounts)
             VALUES ('member_banned_blocked', 'system', 66, 'BeforeBan',
                     '[{\"user_id\":\"66\",\"name\":\"BeforeBan\"}]'::jsonb,
                     '[{\"uuid\":\"00000000-0000-0000-0000-000000000003\",\"name\":\"BeforeBlock\"}]'::jsonb)",
        )
        .execute(&bot)
        .await
        .unwrap();

        let sender = Arc::new(CapturingSender::default());
        AuditDelivery::new(bot, sender.clone(), ChannelId::new(123))
            .deliver_pending()
            .await
            .unwrap();
        let sent = sender.0.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert!(sent[0].title.contains("退出"));
        assert!(sent[0].description.contains("BeforeLeave"));
        assert!(sent[0].description.contains("OldPlayer"));
        assert!(sent[1].title.contains("BAN"));
        assert!(sent[1].description.contains("BeforeBan"));
        assert!(sent[1].description.contains("BeforeBlock"));
    }

    struct FailsFirstSender(Mutex<Vec<i64>>);

    #[async_trait]
    impl AuditSender for &FailsFirstSender {
        async fn send(&self, post: AuditPost) -> Result<()> {
            self.0.lock().unwrap().push(post.event_id);
            if post.event_id == 1 {
                anyhow::bail!("送信失敗");
            }
            Ok(())
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn deletes_only_successful_deliveries(pool: PgPool) {
        sqlx::query(
            "INSERT INTO mcguildlink.audit_logs
             (event_type, actor_type, target_discord_user_id, target_discord_username,
              target_minecraft_uuid, target_minecraft_name)
             VALUES
             ('link_succeeded', 'system', 1, 'first',
              '00000000-0000-0000-0000-000000000001', 'First'),
             ('link_succeeded', 'system', 2, 'second',
              '00000000-0000-0000-0000-000000000002', 'Second')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let sender = FailsFirstSender(Mutex::new(Vec::new()));
        AuditDelivery::new(test_support::bot_pool(&pool).await, &sender, ChannelId::new(123))
            .deliver_pending()
            .await
            .unwrap();
        assert_eq!(*sender.0.lock().unwrap(), vec![1, 2]);
        let remaining = sqlx::query_scalar::<_, i64>("SELECT log_id FROM mcguildlink.audit_outbox")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, vec![1]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcguildlink.audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
    }
}
