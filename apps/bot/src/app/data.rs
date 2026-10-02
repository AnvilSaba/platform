use std::sync::Arc;

use poise::ApplicationContext;
use serenity::all::prelude::Context;
use tokio::sync::RwLock;

use crate::app::{AppApplicationContext, AppContext, AppError, config::AppConfig};

pub struct BotData {
    pub database: sqlx::PgPool,
    config: RwLock<Arc<AppConfig>>,
}

impl BotData {
    pub fn new(config: AppConfig, database: sqlx::PgPool) -> Self {
        Self {
            database,
            config: RwLock::new(Arc::new(config)),
        }
    }

    pub async fn app_config(&self) -> Arc<AppConfig> {
        self.config.read().await.clone()
    }
}

pub trait BotDataExt {
    fn bot_data(&self) -> Arc<BotData>;

    async fn app_config(&self) -> Arc<AppConfig> {
        self.bot_data().app_config().await
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
