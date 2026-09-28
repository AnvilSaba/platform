use super::{
    ports::{CodeIssuanceSession, LinkingRepository},
    queries,
    types::{DiscordAccountId, DiscordUserId, LinkCode},
};
use crate::app::AppError;
use serenity::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Clone)]
pub struct DatabaseLinkingRepository {
    pool: PgPool,
}

impl DatabaseLinkingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub struct DatabaseCodeIssuanceSession {
    transaction: Transaction<'static, Postgres>,
    account: DiscordAccountId,
}

#[async_trait]
impl LinkingRepository for DatabaseLinkingRepository {
    type CodeIssuance = DatabaseCodeIssuanceSession;

    async fn begin_code_issuance(
        &self,
        user_id: DiscordUserId,
        username: &str,
    ) -> Result<Self::CodeIssuance, AppError> {
        let mut transaction = self.pool.begin().await?;
        let account = queries::upsert_discord_account(&mut transaction, user_id, username).await?;
        Ok(DatabaseCodeIssuanceSession { transaction, account })
    }
}

#[async_trait]
impl CodeIssuanceSession for DatabaseCodeIssuanceSession {
    async fn is_blocked(&mut self) -> Result<bool, AppError> {
        Ok(queries::is_discord_blocked(&mut self.transaction, self.account).await?)
    }

    async fn unused_code(&mut self) -> Result<Option<LinkCode>, AppError> {
        Ok(queries::unused_code(&mut self.transaction, self.account).await?)
    }

    async fn reserve_code(&mut self, code: &LinkCode) -> Result<bool, AppError> {
        Ok(queries::reserve_code(&mut self.transaction, self.account, code).await?)
    }

    async fn commit(self) -> Result<(), AppError> {
        self.transaction.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::mcguildlink::{
        linking::{ports::LinkCodeGenerator, ports::LinkCodeResult, service::LinkCodes, types::LinkCode},
        test_support,
    };
    use sqlx::PgPool;
    use std::{collections::VecDeque, sync::Mutex};

    struct FixedCodes(Mutex<VecDeque<&'static str>>);

    impl FixedCodes {
        fn new(codes: &[&'static str]) -> Self {
            Self(Mutex::new(codes.iter().copied().collect()))
        }
    }

    impl LinkCodeGenerator for FixedCodes {
        fn generate(&self) -> LinkCode {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected code generation")
                .into()
        }
    }

    /// セッションの破棄でロールバックする。保存候補を確定せず破棄し、同じ利用者・コードで再発行できることを確認する。
    #[sqlx::test(migrations = "../../migrations")]
    async fn abandoned_session_rolls_back_account_and_code(pool: PgPool) {
        let repository = DatabaseLinkingRepository::new(test_support::bot_pool(&pool).await);
        let code = LinkCode::new("AC234679");
        let mut abandoned = repository
            .begin_code_issuance(DiscordUserId::new(501), "before")
            .await
            .unwrap();
        assert!(abandoned.reserve_code(&code).await.unwrap());
        drop(abandoned);

        let mut next = repository
            .begin_code_issuance(DiscordUserId::new(501), "after")
            .await
            .unwrap();
        assert_eq!(next.unused_code().await.unwrap(), None);
        assert!(next.reserve_code(&code).await.unwrap());
        next.commit().await.unwrap();

        let mut stored = repository
            .begin_code_issuance(DiscordUserId::new(501), "after")
            .await
            .unwrap();
        assert_eq!(stored.unused_code().await.unwrap(), Some(code));
        stored.commit().await.unwrap();
    }

    /// コード衝突時の再生成を確認する。固定生成器で衝突を起こし、両利用者の再表示結果も検証する。
    #[sqlx::test(migrations = "../../migrations")]
    async fn colliding_code_is_retried_without_changing_another_users_code(pool: PgPool) {
        let pool = test_support::bot_pool(&pool).await;
        let first = LinkCodes::with_generator(
            DatabaseLinkingRepository::new(pool.clone()),
            FixedCodes::new(&["AC234679"]),
        );
        assert_eq!(
            first.issue(DiscordUserId::new(10), "first").await.unwrap(),
            LinkCodeResult::Code("AC234679".into())
        );
        let second = LinkCodes::with_generator(
            DatabaseLinkingRepository::new(pool),
            FixedCodes::new(&["AC234679", "KMNPQRTU"]),
        );
        assert_eq!(
            second.issue(DiscordUserId::new(20), "second").await.unwrap(),
            LinkCodeResult::Code("KMNPQRTU".into())
        );
        assert_eq!(
            first.issue(DiscordUserId::new(10), "first").await.unwrap(),
            LinkCodeResult::Code("AC234679".into())
        );
        assert_eq!(
            second.issue(DiscordUserId::new(20), "second").await.unwrap(),
            LinkCodeResult::Code("KMNPQRTU".into())
        );
    }

    /// 同時発行の直列化を確認する。20 件の要求が同じコードを返し、別利用者には異なるコードを発行する。
    #[sqlx::test(migrations = "../../migrations")]
    async fn simultaneous_requests_return_one_reusable_code(pool: PgPool) {
        let service = LinkCodes::new(DatabaseLinkingRepository::new(test_support::bot_pool(&pool).await));
        let results =
            futures::future::join_all((0..20).map(|_| service.issue(DiscordUserId::new(123), "concurrent"))).await;
        let first = results.first().unwrap().as_ref().unwrap();
        assert!(matches!(first, LinkCodeResult::Code(_)));
        for result in &results {
            assert_eq!(result.as_ref().unwrap(), first);
        }
        assert_eq!(&service.issue(DiscordUserId::new(123), "renamed").await.unwrap(), first);
        assert_ne!(
            &service.issue(DiscordUserId::new(124), "different").await.unwrap(),
            first
        );
    }

    /// ブロックによる発行拒否を確認する。既存コードの再表示と、コード削除後の新規発行をともに拒否する。
    #[sqlx::test(migrations = "../../migrations")]
    async fn blocked_user_cannot_issue_or_redisplay_code(pool: PgPool) {
        let service = LinkCodes::new(DatabaseLinkingRepository::new(test_support::bot_pool(&pool).await));
        service.issue(DiscordUserId::new(42), "blocked").await.unwrap();
        test_support::block_test_account(&pool).await;
        assert_eq!(
            service.issue(DiscordUserId::new(42), "blocked").await.unwrap(),
            LinkCodeResult::Blocked
        );
        test_support::remove_test_codes(&pool).await;
        assert_eq!(
            service.issue(DiscordUserId::new(42), "blocked").await.unwrap(),
            LinkCodeResult::Blocked
        );
    }

    /// 未使用コードの永続化を確認する。最大 Discord ID で発行し、サービス再生成・名前変更後も同じコードを返す。
    #[sqlx::test(migrations = "../../migrations")]
    async fn unused_code_survives_reconnection_and_name_change(pool: PgPool) {
        let service = LinkCodes::new(DatabaseLinkingRepository::new(test_support::bot_pool(&pool).await));
        let first = service.issue(DiscordUserId::new(u64::MAX), "before").await.unwrap();
        let LinkCodeResult::Code(code) = &first else {
            panic!("expected code")
        };
        assert_eq!(code.as_ref().len(), 8);
        assert_eq!(
            LinkCodes::new(DatabaseLinkingRepository::new(test_support::bot_pool(&pool).await))
                .issue(DiscordUserId::new(u64::MAX), "after")
                .await
                .unwrap(),
            first
        );
    }
}
