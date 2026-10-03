use super::{
    code_generator::RandomLinkCodeGenerator,
    ports::{CodeIssuanceSession, LinkCodeGenerator, LinkCodeResult, LinkingRepository},
    types::DiscordUserId,
};
use crate::app::AppError;

const MAX_CODE_ALLOCATION_ATTEMPTS: usize = 16;

pub struct LinkCodes<R, G = RandomLinkCodeGenerator> {
    repository: R,
    generator: G,
}

impl<R: LinkingRepository> LinkCodes<R> {
    pub fn new(repository: R) -> Self {
        Self::with_generator(repository, RandomLinkCodeGenerator)
    }
}

impl<R: LinkingRepository, G: LinkCodeGenerator> LinkCodes<R, G> {
    pub fn with_generator(repository: R, generator: G) -> Self {
        Self { repository, generator }
    }

    pub async fn issue(&self, user_id: DiscordUserId, username: &str) -> Result<LinkCodeResult, AppError> {
        let mut session = self.repository.begin_code_issuance(user_id, username).await?;
        let result = self.issue_in_session(&mut session).await?;
        session.commit().await?;
        Ok(result)
    }

    async fn issue_in_session(&self, session: &mut R::CodeIssuance) -> Result<LinkCodeResult, AppError> {
        if session.is_blocked().await? {
            return Ok(LinkCodeResult::Blocked);
        }
        if let Some(code) = session.unused_code().await? {
            return Ok(LinkCodeResult::Code(code));
        }
        for _ in 0..MAX_CODE_ALLOCATION_ATTEMPTS {
            let code = self.generator.generate();
            if session.reserve_code(&code).await? {
                return Ok(LinkCodeResult::Code(code));
            }
        }
        Err(anyhow::anyhow!("Could not allocate a unique link code"))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashSet, VecDeque},
        sync::{Arc, Mutex},
    };

    use serenity::async_trait;

    use super::super::types::LinkCode;
    use super::*;

    #[derive(Clone, Default)]
    struct State {
        blocked: bool,
        code: Option<LinkCode>,
        used_codes: HashSet<String>,
        fail_commit: bool,
    }

    #[derive(Clone, Default)]
    struct MemoryRepository(Arc<Mutex<State>>);

    struct MemoryCodeIssuanceSession {
        store: MemoryRepository,
        pending: State,
    }

    #[async_trait]
    impl LinkingRepository for MemoryRepository {
        type CodeIssuance = MemoryCodeIssuanceSession;
        async fn begin_code_issuance(&self, _: DiscordUserId, _: &str) -> Result<Self::CodeIssuance, AppError> {
            Ok(MemoryCodeIssuanceSession {
                store: self.clone(),
                pending: self.0.lock().unwrap().clone(),
            })
        }
    }

    #[async_trait]
    impl CodeIssuanceSession for MemoryCodeIssuanceSession {
        async fn is_blocked(&mut self) -> Result<bool, AppError> {
            Ok(self.pending.blocked)
        }
        async fn unused_code(&mut self) -> Result<Option<LinkCode>, AppError> {
            Ok(self.pending.code.clone())
        }
        async fn reserve_code(&mut self, code: &LinkCode) -> Result<bool, AppError> {
            if !self.pending.used_codes.insert(code.as_ref().to_owned()) {
                return Ok(false);
            }
            self.pending.code = Some(code.clone());
            Ok(true)
        }
        async fn commit(self) -> Result<(), AppError> {
            if self.pending.fail_commit {
                anyhow::bail!("commit failed");
            }
            *self.store.0.lock().unwrap() = self.pending;
            Ok(())
        }
    }

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

    /// DB なしで衝突時の再生成を確認する。保存済みコードを避け、成功したコードだけを確定する。
    #[tokio::test]
    async fn retries_collision_and_persists_new_code() {
        let store = MemoryRepository::default();
        store.0.lock().unwrap().used_codes.insert("AC234679".into());
        let codes = LinkCodes::with_generator(store.clone(), FixedCodes::new(&["AC234679", "KMNPQRTU"]));
        assert_eq!(
            codes.issue(DiscordUserId::new(42), "user").await.unwrap(),
            LinkCodeResult::Code("KMNPQRTU".into())
        );
        assert_eq!(store.0.lock().unwrap().code, Some("KMNPQRTU".into()));
    }

    /// 未使用コードは再生成しない。生成器に候補を渡さず、保存済みコードの再表示を確認する。
    #[tokio::test]
    async fn reuses_existing_code_without_generation() {
        let store = MemoryRepository::default();
        store.0.lock().unwrap().code = Some("AC234679".into());
        let codes = LinkCodes::with_generator(store, FixedCodes::new(&[]));
        assert_eq!(
            codes.issue(DiscordUserId::new(42), "user").await.unwrap(),
            LinkCodeResult::Code("AC234679".into())
        );
    }

    /// ブロックは既存コードより優先する。保存済みコードがあっても再表示せず、候補も生成しない。
    #[tokio::test]
    async fn rejects_blocked_user_even_with_existing_code() {
        let store = MemoryRepository::default();
        {
            let mut state = store.0.lock().unwrap();
            state.blocked = true;
            state.code = Some("AC234679".into());
        }
        let codes = LinkCodes::with_generator(store, FixedCodes::new(&[]));
        assert_eq!(
            codes.issue(DiscordUserId::new(42), "user").await.unwrap(),
            LinkCodeResult::Blocked
        );
    }

    /// 再試行上限でエラーを返す。同じ衝突を繰り返しても新しいコードは確定しない。
    #[tokio::test]
    async fn collision_exhaustion_does_not_persist_code() {
        let store = MemoryRepository::default();
        store.0.lock().unwrap().used_codes.insert("AC234679".into());
        let codes = LinkCodes::with_generator(
            store.clone(),
            FixedCodes::new(&["AC234679"; MAX_CODE_ALLOCATION_ATTEMPTS]),
        );
        assert!(codes.issue(DiscordUserId::new(42), "user").await.is_err());
        assert_eq!(store.0.lock().unwrap().code, None);
    }

    /// コミット失敗時にはコードを返さない。保存候補ができても永続化に失敗したら発行失敗とする。
    #[tokio::test]
    async fn commit_failure_does_not_return_or_persist_code() {
        let store = MemoryRepository::default();
        store.0.lock().unwrap().fail_commit = true;
        let codes = LinkCodes::with_generator(store.clone(), FixedCodes::new(&["AC234679"]));
        assert!(codes.issue(DiscordUserId::new(42), "user").await.is_err());
        assert_eq!(store.0.lock().unwrap().code, None);
    }
}
