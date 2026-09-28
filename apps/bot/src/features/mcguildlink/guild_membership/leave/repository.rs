use super::{ports::MemberDepartureRepository, queries};
use crate::app::AppError;
use serenity::async_trait;
use sqlx::PgPool;

pub struct DatabaseMemberDepartureRepository {
    pool: PgPool,
}

impl DatabaseMemberDepartureRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MemberDepartureRepository for DatabaseMemberDepartureRepository {
    async fn member_left(&self, user_id: u64, username: &str) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;
        let account_id = queries::lock_discord_account(&mut tx, user_id).await?;
        let Some(account_id) = account_id else {
            return Ok(());
        };
        let targets = queries::audit_targets_for_account(&mut tx, account_id).await?;
        queries::delete_account_links(&mut tx, account_id).await?;
        queries::delete_link_request(&mut tx, account_id).await?;
        for target in &targets {
            queries::record_member_leave(&mut tx, user_id, username, target).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
