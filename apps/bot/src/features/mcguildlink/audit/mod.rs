mod queries;
mod retry;

pub use retry::audit_retry;

pub use queries::resume_stopped;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serenity::{
    all::{ChannelId, CreateMessage, Http},
    async_trait,
};
use sqlx::{PgPool, types::Json};
use tracing::warn;
use uuid::Uuid;

use crate::{
    app::{
        BotData,
        utils::components::{
            create_container, create_container_section, create_container_text, create_section_text,
            create_section_thumbnail, create_separator,
        },
    },
    utils::create_components_v2_message,
};

const MAX_MESSAGE_UNITS: usize = 4_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditPost {
    pub channel_id: ChannelId,
    pub event_id: i64,
    pub occurred_at: DateTime<Utc>,
    pub title: String,
    pub description: String,
    pub actor: String,
    pub thumbnail_uuid: Option<Uuid>,
    pub color: u32,
}

impl AuditPost {
    fn into_message(self) -> CreateMessage<'static> {
        let footer = format!(
            "-# 操作主体: {}\n-# <t:{}:F> · イベントID: {}",
            self.actor,
            self.occurred_at.timestamp(),
            self.event_id
        );
        let max_description_units =
            MAX_MESSAGE_UNITS - self.title.encode_utf16().count() - footer.encode_utf16().count();
        let description = if self.description.encode_utf16().count() > max_description_units {
            let mut shortened = String::new();
            let mut units = 0;
            for ch in self.description.chars() {
                if units + ch.len_utf16() > max_description_units - 12 {
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
        let accounts = match self.thumbnail_uuid {
            Some(uuid) => create_container_section(
                vec![create_section_text(description)],
                create_section_thumbnail(format!("https://mc-heads.net/avatar/{uuid}"), None, false),
            ),
            None => create_container_text(description),
        };
        create_components_v2_message(vec![create_container(
            vec![
                create_container_text(self.title),
                create_separator(false),
                accounts,
                create_separator(false),
                create_container_text(footer),
            ],
            Some(self.color),
            false,
        )])
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
        let target_discord = format!("{} (`{}`)", self.target_discord_username, self.target_discord_user_id);
        let target_minecraft = self
            .target_minecraft_uuid
            .zip(self.target_minecraft_name)
            .map(|(uuid, name)| format!("{name} (`{uuid}`)"));
        let (title, description) = match self.event_type.as_str() {
            "link_succeeded" | "member_leave_unlinked" => (
                if self.event_type == "link_succeeded" {
                    "下記のアカウントの紐付けが完了しました。"
                } else {
                    "メンバーの退出を検知したため、下記アカウントの紐付けを自動解除しました。"
                }
                .to_owned(),
                format!(
                    "- Discordユーザー\n  - {target_discord}\n- Minecraftアカウント\n  - {}",
                    target_minecraft.unwrap_or_default()
                ),
            ),
            "member_banned_blocked" => {
                let mut description = format!("- BAN対象の Discordユーザー\n  - {target_discord}");
                if let Some(accounts) = self.related_discord_accounts {
                    description.push_str("\n\n- ブロックした Discordアカウント");
                    for account in accounts.0 {
                        description.push_str(&format!("\n  - {} (`{}`)", account.name, account.user_id));
                    }
                }
                if let Some(accounts) = self.related_minecraft_accounts {
                    description.push_str("\n\n- ブロックした Minecraftアカウント");
                    for account in accounts.0 {
                        description.push_str(&format!("\n  - {} (`{}`)", account.name, account.uuid));
                    }
                }
                (
                    "メンバーのBANを検知したため、関連アカウントを自動ブロックしました。".to_owned(),
                    description,
                )
            }
            _ => (
                format!("監査イベント: {}", self.event_type),
                format!("- Discordユーザー\n  - {target_discord}"),
            ),
        };
        let actor = match self.actor_type.as_str() {
            "minecraft_player" => self
                .actor_minecraft_uuid
                .zip(self.actor_minecraft_name)
                .map(|(uuid, name)| format!("Minecraft {name} (`{uuid}`)"))
                .unwrap_or_else(|| "Minecraft".to_owned()),
            "discord_member" => self
                .actor_discord_user_id
                .zip(self.actor_discord_username)
                .map(|(id, name)| format!("Discord {name} (`{id}`)"))
                .unwrap_or_else(|| "Discord".to_owned()),
            "system" => "システム".to_owned(),
            _ => self.actor_type,
        };
        AuditPost {
            channel_id,
            event_id: self.id,
            occurred_at: self.occurred_at,
            title,
            description,
            actor,
            thumbnail_uuid: (self.event_type == "link_succeeded")
                .then_some(self.target_minecraft_uuid)
                .flatten(),
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
        let pending = queries::pending(&self.pool).await?;
        for event in pending {
            let id = event.id;
            if let Err(error) = self.sender.send(event.post(self.channel_id)).await {
                warn!(event_id = id, "監査ログの配送に失敗しました: {error:#}");
                let needs_attention = error.downcast_ref::<serenity::Error>().is_some_and(|error| {
                    matches!(error, serenity::Error::Http(error)
                        if error.status_code().is_some_and(|status| matches!(status.as_u16(), 403 | 404)))
                });
                queries::record_failure(&self.pool, id, needs_attention).await?;
                continue;
            }
            queries::delete_delivered(&self.pool, id).await?;
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
            warn!("監査ログの配送状態の読み取りまたは更新に失敗しました: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::mcguildlink::test_support;
    use std::{sync::Mutex, time::Duration};

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

        let message = {
            let sent = sender.0.lock().unwrap();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].channel_id, ChannelId::new(123));
            assert!(sent[0].description.contains("DiscordOld"));
            assert!(sent[0].description.contains("AliceOld"));
            assert_eq!(sent[0].event_id, 1);
            assert_eq!(sent[0].occurred_at.timestamp(), 1_790_685_296);
            serde_json::to_value(sent[0].clone().into_message()).unwrap()
        };
        assert_eq!(message["flags"], 32768);
        assert!(message["content"].as_str().unwrap_or_default().is_empty());
        assert!(message["embeds"].as_array().is_none_or(Vec::is_empty));
        assert_eq!(message["allowed_mentions"]["parse"], serde_json::json!([]));
        let container = &message["components"][0];
        assert_eq!(container["type"], 17);
        assert_eq!(container["accent_color"], 0x57F287);
        let components = container["components"].as_array().unwrap();
        assert_eq!(components.len(), 5);
        assert_eq!(components[0]["content"], "下記のアカウントの紐付けが完了しました。");
        assert_eq!(components[1]["type"], 14);
        assert_eq!(components[1]["divider"], false);
        assert_eq!(components[2]["type"], 9);
        assert_eq!(
            components[2]["components"][0]["content"],
            "- Discordユーザー\n  - DiscordOld (`42`)\n- Minecraftアカウント\n  - AliceOld (`00000000-0000-0000-0000-000000000001`)"
        );
        assert_eq!(components[2]["accessory"]["type"], 11);
        assert_eq!(
            components[2]["accessory"]["media"]["url"],
            "https://mc-heads.net/avatar/00000000-0000-0000-0000-000000000001"
        );
        assert_eq!(components[3]["type"], 14);
        assert_eq!(components[3]["divider"], false);
        assert_eq!(
            components[4]["content"],
            "-# 操作主体: Minecraft AliceOld (`00000000-0000-0000-0000-000000000001`)\n-# <t:1790685296:F> · イベントID: 1"
        );
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
        let lock_pid = sqlx::query_scalar::<_, i32>("SELECT pg_backend_pid() FROM mcguildlink.audit_outbox FOR UPDATE")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        let sender = Arc::new(CapturingSender::default());
        let delivery = AuditDelivery::new(test_support::bot_pool(&pool).await, sender.clone(), ChannelId::new(123));
        let task = tokio::spawn(async move { delivery.deliver_pending().await });
        let delete_pid = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(pid) = sqlx::query_scalar::<_, i32>(
                    "SELECT pid FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid))",
                )
                .bind(lock_pid)
                .fetch_optional(&pool)
                .await
                .unwrap()
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // タスクの abort だけでは送信済み SQL は止まらない。DB 接続を切って停止を再現する。
        assert!(
            sqlx::query_scalar::<_, bool>("SELECT pg_terminate_backend($1, 5000)")
                .bind(delete_pid)
                .fetch_one(&pool)
                .await
                .unwrap()
        );
        assert!(task.await.unwrap().is_err());
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
        {
            let first = sender.0.lock().unwrap();
            let second = retry_sender.0.lock().unwrap();
            assert_eq!(first.len(), 1);
            assert_eq!(second.len(), 1);
            assert_eq!(first[0].event_id, second[0].event_id);
            assert_eq!(second[0].channel_id, ChannelId::new(456));
        }
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
        for post in sent.iter() {
            let message = serde_json::to_value(post.clone().into_message()).unwrap();
            assert_eq!(message["flags"], 32768);
            let components = message["components"][0]["components"].as_array().unwrap();
            assert_eq!(components.len(), 5);
            assert_eq!(components[0]["content"], post.title);
            assert_eq!(components[1]["divider"], false);
            assert_eq!(components[2]["type"], 10);
            assert_eq!(components[2]["content"], post.description);
            assert_eq!(components[3]["divider"], false);
        }
        assert_eq!(
            sent[0].title,
            "メンバーの退出を検知したため、下記アカウントの紐付けを自動解除しました。"
        );
        assert_eq!(
            sent[0].description,
            "- Discordユーザー\n  - BeforeLeave (`55`)\n- Minecraftアカウント\n  - OldPlayer (`00000000-0000-0000-0000-000000000002`)"
        );
        assert_eq!(
            sent[1].title,
            "メンバーのBANを検知したため、関連アカウントを自動ブロックしました。"
        );
        assert_eq!(
            sent[1].description,
            "- BAN対象の Discordユーザー\n  - BeforeBan (`66`)\n\n- ブロックした Discordアカウント\n  - BeforeBan (`66`)\n\n- ブロックした Minecraftアカウント\n  - BeforeBlock (`00000000-0000-0000-0000-000000000003`)"
        );
        let mut large_post = sent[1].clone();
        large_post.description = "😀".repeat(4_000);
        let message = serde_json::to_value(large_post.into_message()).unwrap();
        let components = message["components"][0]["components"].as_array().unwrap();
        let total_units: usize = components
            .iter()
            .filter_map(|component| component["content"].as_str())
            .map(|content| content.encode_utf16().count())
            .sum();
        assert!(total_units <= MAX_MESSAGE_UNITS);
        assert!(components[2]["content"].as_str().unwrap().ends_with("…（以下省略）"));
    }

    struct FailsFirstSender(Mutex<Vec<i64>>);

    struct HttpFailureSender;

    #[async_trait]
    impl AuditSender for HttpFailureSender {
        async fn send(&self, post: AuditPost) -> Result<()> {
            let status = match post.event_id {
                1 => 403,
                2 => 404,
                3 => 429,
                4 => 500,
                _ => return Ok(()),
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let app = axum::Router::new().fallback(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    axum::Json(serde_json::json!({"code": 0, "message": "配送失敗"})),
                )
            });
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let http = serenity::http::HttpBuilder::without_token()
                .proxy(format!("http://{address}"))
                .ratelimiter_disabled(true)
                .build();
            let result = DiscordAuditSender::new(Arc::new(http)).send(post).await;
            server.abort();
            result
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn stops_permanent_failures_and_resumes_selected_or_all(pool: PgPool) {
        sqlx::query(
            "INSERT INTO mcguildlink.audit_logs
             (event_type, actor_type, target_discord_user_id, target_discord_username,
              target_minecraft_uuid, target_minecraft_name)
             SELECT 'member_leave_unlinked', 'system', n, 'user',
                    '00000000-0000-0000-0000-000000000001', 'Player'
             FROM generate_series(1, 5) n",
        )
        .execute(&pool)
        .await
        .unwrap();
        let bot = test_support::bot_pool(&pool).await;
        AuditDelivery::new(bot.clone(), HttpFailureSender, ChannelId::new(123))
            .deliver_pending()
            .await
            .unwrap();
        let states = sqlx::query_as::<_, (i64, bool, i32, bool)>(
            "SELECT log_id, needs_attention, retry_count, next_attempt_at > now()
             FROM mcguildlink.audit_outbox ORDER BY log_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            states,
            vec![
                (1, true, 1, true),
                (2, true, 1, true),
                (3, false, 1, true),
                (4, false, 1, true)
            ]
        );
        let sender = Arc::new(CapturingSender::default());
        AuditDelivery::new(bot.clone(), sender.clone(), ChannelId::new(456))
            .deliver_pending()
            .await
            .unwrap();
        assert!(sender.0.lock().unwrap().is_empty());
        assert_eq!(resume_stopped(&bot, Some(3)).await.unwrap(), 0);
        assert_eq!(resume_stopped(&bot, Some(999)).await.unwrap(), 0);
        assert_eq!(resume_stopped(&bot, Some(1)).await.unwrap(), 1);
        AuditDelivery::new(bot.clone(), sender.clone(), ChannelId::new(456))
            .deliver_pending()
            .await
            .unwrap();
        assert_eq!(
            sender.0.lock().unwrap().iter().map(|p| p.event_id).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(resume_stopped(&bot, None).await.unwrap(), 1);
        AuditDelivery::new(bot, sender.clone(), ChannelId::new(456))
            .deliver_pending()
            .await
            .unwrap();
        assert_eq!(
            sender.0.lock().unwrap().iter().map(|p| p.event_id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(resume_stopped(&pool, None).await.unwrap(), 0);
    }

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
        let (retry_count, delayed, needs_attention) = sqlx::query_as::<_, (i32, bool, bool)>(
            "SELECT retry_count, next_attempt_at >= now() + interval '50 seconds', needs_attention
             FROM mcguildlink.audit_outbox WHERE log_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((retry_count, delayed, needs_attention), (1, true, false));
        // 再生成した配送役でも、保存した再試行時刻までは送信しない。
        AuditDelivery::new(test_support::bot_pool(&pool).await, &sender, ChannelId::new(123))
            .deliver_pending()
            .await
            .unwrap();
        assert_eq!(*sender.0.lock().unwrap(), vec![1, 2]);
        sqlx::query("UPDATE mcguildlink.audit_outbox SET next_attempt_at = now() WHERE log_id = 1")
            .execute(&pool)
            .await
            .unwrap();
        AuditDelivery::new(test_support::bot_pool(&pool).await, &sender, ChannelId::new(123))
            .deliver_pending()
            .await
            .unwrap();
        let state = sqlx::query_as::<_, (i32, bool)>(
            "SELECT retry_count, next_attempt_at >= now() + interval '110 seconds'
             FROM mcguildlink.audit_outbox WHERE log_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, (2, true));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mcguildlink.audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
    }
}
