use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context as _;
use poise::ApplicationContext;
use serenity::all::prelude::Context;
use tokio::sync::RwLock;

use crate::app::{AppApplicationContext, AppContext, AppError, config::AppConfig, config_watcher::ConfigWatcher};

pub struct BotData {
    pub database: sqlx::PgPool,
    config_path: PathBuf,
    keyword_path: Option<PathBuf>,
    config: RwLock<RuntimeConfig>,
}

struct RuntimeConfig {
    app: Arc<AppConfig>,
    auth_keyword: String,
}

async fn read_auth_keyword(keyword_path: Option<&Path>) -> Result<String, AppError> {
    let keyword = match keyword_path {
        Some(path) => tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read keyword file: {}", path.display()))?,
        None => std::env::var("AUTH_KEYWORD").context("AUTH_KEYWORD or --keyword-file must be set")?,
    };
    parse_auth_keyword(&keyword)
}

fn parse_auth_keyword(keyword: &str) -> Result<String, AppError> {
    let keyword = keyword.trim();
    anyhow::ensure!(!keyword.is_empty(), "Authentication keyword must not be empty");
    Ok(keyword.to_owned())
}

impl BotData {
    pub async fn new(
        config: AppConfig,
        database: sqlx::PgPool,
        config_path: PathBuf,
        keyword_path: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        let config = RwLock::new(RuntimeConfig {
            app: Arc::new(config),
            auth_keyword: read_auth_keyword(keyword_path.as_deref()).await?,
        });

        Ok(Self {
            database,
            config_path,
            keyword_path,
            config,
        })
    }

    pub async fn app_config(&self) -> Arc<AppConfig> {
        self.config.read().await.app.clone()
    }

    pub async fn auth_keyword_matches(&self, submitted: &str) -> bool {
        self.config.read().await.auth_keyword == submitted
    }

    async fn reload_config(self: Arc<Self>, contents: &str) -> Result<(), AppError> {
        let config = AppConfig::parse(contents)?;
        self.config.write().await.app = Arc::new(config);
        Ok(())
    }

    async fn reload_auth_keyword(self: Arc<Self>, contents: &str) -> Result<(), AppError> {
        let keyword = parse_auth_keyword(contents)?;
        self.config.write().await.auth_keyword = keyword;
        Ok(())
    }

    pub fn watch_config(self: Arc<Self>) -> ConfigWatcher {
        let mut watcher = ConfigWatcher::default();

        let data = self.clone();
        watcher.register(&self.config_path, move |c| Box::pin(data.clone().reload_config(c)));

        if let Some(path) = &self.keyword_path {
            let data = self.clone();
            watcher.register(path, move |c| Box::pin(data.clone().reload_auth_keyword(c)));
        }

        watcher
    }
}

pub trait BotDataExt {
    fn bot_data(&self) -> Arc<BotData>;

    async fn app_config(&self) -> Arc<AppConfig> {
        self.bot_data().app_config().await
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
