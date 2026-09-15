use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use crate::features::discord_management::ids::GuildId;

/// Discord 管理の apply を Guild 単位で排他する、プロセス内の共有ロックです。
#[derive(Clone, Default)]
pub(crate) struct GuildApplyLock {
    applying_guilds: Arc<Mutex<BTreeSet<GuildId>>>,
}

impl GuildApplyLock {
    pub(crate) fn try_acquire(&self, guild_id: GuildId) -> Option<GuildApplyPermit> {
        let mut guilds = self.applying_guilds.lock().expect("guild apply mutex poisoned");
        guilds.insert(guild_id).then(|| GuildApplyPermit {
            lock: self.clone(),
            guild_id,
        })
    }
}

/// 保持している間、対象 Guild に別の apply が入ることを防ぎます。
pub(crate) struct GuildApplyPermit {
    lock: GuildApplyLock,
    guild_id: GuildId,
}

impl Drop for GuildApplyPermit {
    fn drop(&mut self) {
        self.lock
            .applying_guilds
            .lock()
            .expect("guild apply mutex poisoned")
            .remove(&self.guild_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_guild_is_locked_until_the_permit_is_dropped() {
        let lock = GuildApplyLock::default();
        let permit = lock
            .try_acquire(GuildId::new(100))
            .expect("first apply acquires the Guild");

        assert!(lock.try_acquire(GuildId::new(100)).is_none());
        drop(permit);
        assert!(lock.try_acquire(GuildId::new(100)).is_some());
    }

    #[test]
    fn different_guilds_can_be_acquired_at_the_same_time() {
        let lock = GuildApplyLock::default();
        let _first = lock.try_acquire(GuildId::new(100)).expect("first Guild is available");

        assert!(lock.try_acquire(GuildId::new(200)).is_some());
    }
}
