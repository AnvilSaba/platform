use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const CONFIRMATION_TTL: Duration = Duration::from_secs(5 * 60);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ConfirmationToken(String);

impl ConfirmationToken {
    pub fn custom_id(&self) -> String {
        format!("discord-management:role-apply:{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmationError {
    Unknown,
    WrongOwner,
    Expired,
    AlreadyConsumed,
}

struct PendingConfirmation<T> {
    owner_id: u64,
    expires_at: Instant,
    payload: Option<T>,
}

pub struct ConfirmationStore<T> {
    entries: Mutex<HashMap<ConfirmationToken, PendingConfirmation<T>>>,
}

impl<T> Default for ConfirmationStore<T> {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl<T> ConfirmationStore<T> {
    pub fn issue(&self, owner_id: u64, payload: T, now: Instant) -> ConfirmationToken {
        let token = ConfirmationToken(NEXT_TOKEN.fetch_add(1, Ordering::Relaxed).to_string());
        self.entries.lock().expect("confirmation store mutex poisoned").insert(
            token.clone(),
            PendingConfirmation {
                owner_id,
                expires_at: now + CONFIRMATION_TTL,
                payload: Some(payload),
            },
        );
        token
    }

    pub fn consume(&self, token: &ConfirmationToken, owner_id: u64, now: Instant) -> Result<T, ConfirmationError> {
        let mut entries = self.entries.lock().expect("confirmation store mutex poisoned");
        let pending = entries.get_mut(token).ok_or(ConfirmationError::Unknown)?;
        if pending.owner_id != owner_id {
            return Err(ConfirmationError::WrongOwner);
        }
        if now >= pending.expires_at {
            return Err(ConfirmationError::Expired);
        }
        pending.payload.take().ok_or(ConfirmationError::AlreadyConsumed)
    }
}
