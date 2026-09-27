mod config;
mod identity;
mod link_flow;
mod link_store;
mod protocol;
mod server;
mod session;

use std::{
    error::Error,
    io::{self, ErrorKind},
    sync::Arc,
};
use tokio::net::TcpListener;

use crate::{config::AppConfig, link_store::LinkStore, server::LinkServer, session::MojangVerifier};

const VERSION: i32 = 777;
type AppResult<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn invalid(reason: &'static str) -> Box<dyn Error + Send + Sync> {
    io::Error::new(ErrorKind::InvalidData, reason).into()
}

#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() -> AppResult<()> {
    let config = AppConfig::from_file("config.toml").await?;
    let linker = LinkStore::connect().await?;
    let server = Arc::new(LinkServer::new(MojangVerifier::new()?, linker));
    let listener = TcpListener::bind(config.server.listen).await?;
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
    platform_signal::install_signal_handler(move || {
        let _ = shutdown_tx.send(());
    })?;
    eprintln!("MCGuildLink 26.3 listening on {}", config.server.listen);
    loop {
        let (stream, _) = tokio::select! {
            result = listener.accept() => result?,
            _ = &mut shutdown_rx => break,
        };
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            if let Err(error) = server.serve(stream).await {
                eprintln!("Minecraft connection ended: {error}");
            }
        });
    }
    Ok(())
}
