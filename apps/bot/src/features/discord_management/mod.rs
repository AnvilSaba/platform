mod adapter;
mod command;
mod confirmation;
mod ids;
mod service;

pub use command::{role_apply, role_export, role_plan};

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Barrier},
        time::{Duration, Instant},
    };

    use super::confirmation::{ConfirmationError, ConfirmationStore};

    /// 確認トークンを発行者だけが期限内に一度だけ消費でき、再利用や期限切れを拒否することを保証する。
    #[test]
    fn confirmation_is_owner_only_expires_after_five_minutes_and_is_consumed_once() {
        let confirmations = ConfirmationStore::default();
        let now = Instant::now();
        let token = confirmations.issue(10, "payload", now);

        assert_eq!(
            confirmations.consume(&token, 11, now),
            Err(ConfirmationError::WrongOwner)
        );
        assert_eq!(confirmations.consume(&token, 10, now), Ok("payload"));
        assert_eq!(
            confirmations.consume(&token, 10, now),
            Err(ConfirmationError::AlreadyConsumed)
        );

        let expired = confirmations.issue(10, "expired", now);
        assert_eq!(
            confirmations.consume(&expired, 10, now + Duration::from_secs(300)),
            Err(ConfirmationError::Expired)
        );
    }

    /// 同じ確認ボタンが同時に押されても一方だけが成功し、Role変更が二重適用されないことを保証する。
    #[test]
    fn concurrent_confirmation_clicks_have_exactly_one_winner() {
        let confirmations = Arc::new(ConfirmationStore::default());
        let now = Instant::now();
        let token = confirmations.issue(10, "payload", now);
        let barrier = Arc::new(Barrier::new(3));
        let attempts = (0..2)
            .map(|_| {
                let confirmations = confirmations.clone();
                let token = token.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    confirmations.consume(&token, 10, now)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = attempts
            .into_iter()
            .map(|attempt| attempt.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Err(ConfirmationError::AlreadyConsumed))
                .count(),
            1
        );
    }
}
