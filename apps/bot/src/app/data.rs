use std::sync::Arc;

use poise::ApplicationContext;
use serenity::all::prelude::Context;
use tokio::sync::RwLock;

use crate::app::{AppApplicationContext, AppContext, AppError, config::AppConfig};
use crate::features::discord_management::GuildApplyLock;

pub struct BotData {
    config: RwLock<Arc<AppConfig>>,
    discord_management_apply_lock: GuildApplyLock,
}

impl BotData {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: RwLock::new(Arc::new(config)),
            discord_management_apply_lock: GuildApplyLock::default(),
        }
    }

    pub(crate) fn discord_management_apply_lock(&self) -> &GuildApplyLock {
        &self.discord_management_apply_lock
    }
}

pub trait BotDataExt {
    fn bot_data(&self) -> Arc<BotData>;

    async fn app_config(&self) -> Arc<AppConfig> {
        self.bot_data().config.read().await.clone()
    }

    async fn replace_app_config(&self, config: AppConfig) {
        let data = self.bot_data();
        *data.config.write().await = Arc::new(config);
    }
}

impl BotDataExt for Context {
    fn bot_data(&self) -> Arc<BotData> {
        self.data()
    }
}

impl<'a> BotDataExt for AppContext<'a> {
    fn bot_data(&self) -> Arc<BotData> {
        self.data()
    }
}

impl<'a> BotDataExt for AppApplicationContext<'a> {
    fn bot_data(&self) -> Arc<BotData> {
        self.data()
    }
}
