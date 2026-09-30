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
    let timeout = match std::env::var("MC_LINK_SERVER_INPUT_TIMEOUT_SECONDS") {
        Ok(value) => {
            let seconds: f64 = value.parse()?;
            let duration = std::time::Duration::try_from_secs_f64(seconds)?;
            if duration.is_zero() {
                return Err(invalid("input timeout must be positive"));
            }
            duration
        }
        Err(std::env::VarError::NotPresent) => link_flow::INPUT_TIMEOUT,
        Err(error) => return Err(error.into()),
    };
    let linker = LinkStore::connect().await?;
    let server = Arc::new(LinkServer::new(MojangVerifier::new()?, linker, timeout));
    let listener = TcpListener::bind(address).await?;
    eprintln!("MC Link Server 26.3 listening on {}", address);
    loop {
        let (stream, _) = listener.accept().await?;
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            if let Err(error) = server.serve(stream).await {
                eprintln!("Minecraft connection ended: {error}");
            }
        });
    }
}
