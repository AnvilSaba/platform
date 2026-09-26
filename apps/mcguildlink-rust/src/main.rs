mod link_flow;
mod protocol;
mod server;
mod session;

use std::{
    error::Error,
    io::{self, ErrorKind},
    net::TcpListener,
};

const VERSION: i32 = 777;
type AppResult<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn invalid(reason: &'static str) -> Box<dyn Error + Send + Sync> {
    io::Error::new(ErrorKind::InvalidData, reason).into()
}

#[cfg(test)]
mod tests;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("MCGUILDLINK_LISTEN").unwrap_or_else(|_| "127.0.0.1:25565".into());
    let listener = TcpListener::bind(&address)?;
    eprintln!("MCGuildLink 26.3 listening on {address}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(move || {
                    if let Err(error) = server::serve(stream) {
                        eprintln!("Minecraft connection ended: {error}");
                    }
                });
            }
            Err(error) => eprintln!("Minecraft accept failed: {error}"),
        }
    }
    Ok(())
}
