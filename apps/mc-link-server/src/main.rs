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

use crate::{link_store::LinkStore, server::LinkServer, session::MojangVerifier};

const VERSION: i32 = 777;
type AppResult<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn invalid(reason: &'static str) -> Box<dyn Error + Send + Sync> {
    io::Error::new(ErrorKind::InvalidData, reason).into()
}

#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() -> AppResult<()> {
    let address: std::net::SocketAddr = std::env::var("MC_LINK_SERVER_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:25565".into())
        .parse()?;
    let linker = LinkStore::connect().await?;
    let server = Arc::new(LinkServer::new(MojangVerifier::new()?, linker));
    let listener = TcpListener::bind(address).await?;
    let mut shutdown_rx = platform_signal::shutdown_receiver()?;
    eprintln!("MC Link Server 26.3 listening on {}", address);
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
