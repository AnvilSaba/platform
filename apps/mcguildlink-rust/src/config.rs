use std::net::SocketAddr;

use serde::Deserialize;

use crate::AppResult;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppConfig {
    pub(crate) server: ServerConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServerConfig {
    pub(crate) listen: SocketAddr,
}

impl AppConfig {
    pub(crate) async fn from_file(path: &str) -> AppResult<Self> {
        let text = tokio::fs::read_to_string(path).await?;
        Ok(toml::from_str(&text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_address_comes_from_toml() {
        let config: AppConfig = toml::from_str("[server]\nlisten = '127.0.0.1:25565'").unwrap();
        assert_eq!(config.server.listen, "127.0.0.1:25565".parse().unwrap());
        assert!(toml::from_str::<AppConfig>("[server]\nlisten = 'invalid'").is_err());
    }
}
