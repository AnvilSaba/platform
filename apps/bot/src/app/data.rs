use std::sync::Arc;

use poise::ApplicationContext;
use serenity::all::prelude::Context;
use tokio::sync::RwLock;

use crate::{
    app::{AppApplicationContext, AppContext, AppError, config::AppConfig},
    features::mcguildlink::LinkManagement,
};

pub struct BotData {
    pub database: sqlx::PgPool,
    pub link_management: LinkManagement,
    config: RwLock<Arc<AppConfig>>,
}

impl BotData {
    pub fn new(config: AppConfig, database: sqlx::PgPool, link_management: LinkManagement) -> Self {
        Self {
            database,
            link_management,
            config: RwLock::new(Arc::new(config)),
        }
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
