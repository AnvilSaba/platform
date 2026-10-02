use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct DiscordAccount {
    pub user_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MinecraftAccount {
    pub uuid: Uuid,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct BlockGroup {
    pub root: DiscordAccount,
    pub discord: Vec<DiscordAccount>,
    pub minecraft: Vec<MinecraftAccount>,
    pub created_at: DateTime<Utc>,
}

pub enum BlockResult {
    Blocked(BlockGroup),
    AlreadyBlocked,
}

#[derive(Clone, Copy)]
pub enum BlockCause {
    Moderator,
    GuildBan,
}

#[derive(Clone)]
pub struct DatabaseBlockRepository {
    pool: PgPool,
}

impl DatabaseBlockRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn block(&self, user_id: u64, username: &str, cause: BlockCause) -> Result<BlockResult, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT mcguildlink.serialize_account_changes()")
            .execute(&mut *tx)
            .await?;
        // 発行処理と同じ Discord 行をロックし、同じ利用者の操作を直列化する。
        let root_id: i64 = sqlx::query_scalar(
            "INSERT INTO mcguildlink.discord_accounts (user_id, last_known_username) VALUES ($1::text::numeric, $2) \
             ON CONFLICT (user_id) DO UPDATE SET last_known_username = EXCLUDED.last_known_username RETURNING id",
        )
        .bind(user_id.to_string())
        .bind(username)
        .fetch_one(&mut *tx)
        .await?;

        // Discord と Minecraft の二部グラフをたどり、多対多の連鎖全体を収集する。
        let rows = sqlx::query(
            "WITH RECURSIVE edges(from_kind, from_id, to_kind, to_id) AS ( \
               SELECT 0, discord_account_id, 1, minecraft_account_id FROM mcguildlink.account_links \
               UNION ALL SELECT 1, minecraft_account_id, 0, discord_account_id FROM mcguildlink.account_links \
             ), walk(kind, id) AS ( \
               SELECT 0, $1::bigint UNION SELECT e.to_kind, e.to_id FROM walk w \
               JOIN edges e ON e.from_kind = w.kind AND e.from_id = w.id \
             ) SELECT kind, id FROM walk ORDER BY kind, id",
        )
        .bind(root_id)
        .fetch_all(&mut *tx)
        .await?;
        let mut discord_ids = Vec::new();
        let mut minecraft_ids = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("id")?;
            if row.try_get::<i32, _>("kind")? == 0 {
                discord_ids.push(id);
            } else {
                minecraft_ids.push(id);
            }
        }
        let already_blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT FROM mcguildlink.blocked_discord_accounts WHERE discord_account_id = ANY($1)) \
                 OR EXISTS (SELECT FROM mcguildlink.blocked_minecraft_accounts WHERE minecraft_account_id = ANY($2))",
        )
        .bind(&discord_ids)
        .bind(&minecraft_ids)
        .fetch_one(&mut *tx)
        .await?;
        if already_blocked {
            return Ok(BlockResult::AlreadyBlocked);
        }

        let discord = account_discord(&mut tx, &discord_ids).await?;
        let minecraft = account_minecraft(&mut tx, &minecraft_ids).await?;
        let (group_id, created_at): (i64, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO mcguildlink.block_groups (root_discord_account_id) VALUES ($1) RETURNING id, created_at",
        )
        .bind(root_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO mcguildlink.blocked_discord_accounts (discord_account_id, block_group_id) \
             SELECT unnest($1::bigint[]), $2",
        )
        .bind(&discord_ids)
        .bind(group_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO mcguildlink.blocked_minecraft_accounts (minecraft_account_id, block_group_id) \
             SELECT unnest($1::bigint[]), $2",
        )
        .bind(&minecraft_ids)
        .bind(group_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM mcguildlink.account_links WHERE discord_account_id = ANY($1) OR minecraft_account_id = ANY($2)")
            .bind(&discord_ids)
            .bind(&minecraft_ids)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM mcguildlink.link_requests WHERE discord_account_id = ANY($1)")
            .bind(&discord_ids)
            .execute(&mut *tx)
            .await?;

        let group = BlockGroup {
            root: discord
                .iter()
                .find(|account| account.user_id == user_id.to_string())
                .expect("root exists")
                .clone(),
            discord,
            minecraft,
            created_at,
        };
        if matches!(cause, BlockCause::GuildBan) {
            sqlx::query(
                "INSERT INTO mcguildlink.audit_logs \
                 (event_type, actor_type, target_discord_user_id, target_discord_username, \
                  related_discord_accounts, related_minecraft_accounts) \
                 VALUES ('member_banned_blocked', 'system', $1::text::numeric, $2, $3, $4)",
            )
            .bind(user_id.to_string())
            .bind(username)
            .bind(sqlx::types::Json(&group.discord))
            .bind(sqlx::types::Json(&group.minecraft))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(BlockResult::Blocked(group))
    }

    pub async fn unblock(&self, user_id: u64) -> Result<Option<BlockGroup>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let group = sqlx::query(
            "SELECT g.id, g.created_at, root.user_id::text AS root_user_id, root.last_known_username AS root_name \
             FROM mcguildlink.block_groups g \
             JOIN mcguildlink.discord_accounts root ON root.id = g.root_discord_account_id \
             JOIN mcguildlink.blocked_discord_accounts b ON b.block_group_id = g.id \
             JOIN mcguildlink.discord_accounts target ON target.id = b.discord_account_id \
             WHERE target.user_id = $1::text::numeric",
        )
        .bind(user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(group) = group else { return Ok(None) };
        let group_id: i64 = group.try_get("id")?;
        let snapshot = group_snapshot(
            &mut tx,
            group_id,
            group.try_get("created_at")?,
            DiscordAccount {
                user_id: group.try_get("root_user_id")?,
                name: group.try_get("root_name")?,
            },
        )
        .await?;
        sqlx::query("DELETE FROM mcguildlink.blocked_discord_accounts WHERE block_group_id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM mcguildlink.blocked_minecraft_accounts WHERE block_group_id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM mcguildlink.block_groups WHERE id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(snapshot))
    }

    pub async fn list(&self) -> Result<Vec<BlockGroup>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            "SELECT g.id, g.created_at, root.user_id::text AS root_user_id, root.last_known_username AS root_name \
             FROM mcguildlink.block_groups g JOIN mcguildlink.discord_accounts root \
             ON root.id = g.root_discord_account_id ORDER BY g.created_at DESC, g.id DESC",
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut groups = Vec::with_capacity(rows.len());
        for row in rows {
            groups.push(
                group_snapshot(
                    &mut tx,
                    row.try_get("id")?,
                    row.try_get("created_at")?,
                    DiscordAccount {
                        user_id: row.try_get("root_user_id")?,
                        name: row.try_get("root_name")?,
                    },
                )
                .await?,
            );
        }
        tx.commit().await?;
        Ok(groups)
    }
}

async fn account_discord(tx: &mut Transaction<'_, Postgres>, ids: &[i64]) -> Result<Vec<DiscordAccount>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT user_id::text AS user_id, last_known_username AS name FROM mcguildlink.discord_accounts \
         WHERE id = ANY($1) ORDER BY user_id",
    )
    .bind(ids)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(DiscordAccount {
                user_id: row.try_get("user_id")?,
                name: row.try_get("name")?,
            })
        })
        .collect()
}

async fn account_minecraft(
    tx: &mut Transaction<'_, Postgres>,
    ids: &[i64],
) -> Result<Vec<MinecraftAccount>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT uuid, last_known_name AS name FROM mcguildlink.minecraft_accounts \
         WHERE id = ANY($1) ORDER BY uuid",
    )
    .bind(ids)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(MinecraftAccount {
                uuid: row.try_get("uuid")?,
                name: row.try_get("name")?,
            })
        })
        .collect()
}

async fn group_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
    created_at: DateTime<Utc>,
    root: DiscordAccount,
) -> Result<BlockGroup, sqlx::Error> {
    let discord_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT discord_account_id FROM mcguildlink.blocked_discord_accounts WHERE block_group_id = $1",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let minecraft_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT minecraft_account_id FROM mcguildlink.blocked_minecraft_accounts WHERE block_group_id = $1",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let mut discord = account_discord(tx, &discord_ids).await?;
    discord.sort_by_key(|account| (account.user_id != root.user_id, account.user_id.clone()));
    Ok(BlockGroup {
        root,
        discord,
        minecraft: account_minecraft(tx, &minecraft_ids).await?,
        created_at,
    })
}
