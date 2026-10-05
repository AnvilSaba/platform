use std::{
    io::{self, Cursor, ErrorKind},
    time::Duration,
};

use mc_protocol::{
    packet::{PacketError, PacketId, RawPacket, UncompressedPacket},
    ser::{Deserialize, Serialize},
    varint::VarIntError,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    time::{Instant, interval, sleep_until, timeout},
};

use super::{crypto::CryptoStream, packets::ConfigurationKeepAlive};
use crate::{AppResult, invalid};

pub(crate) fn decode_exact<T: Deserialize>(payload: &[u8]) -> AppResult<T> {
    let mut reader = Cursor::new(payload);
    let value = T::deserialize(&mut reader)?;
    if reader.position() != payload.len() as u64 {
        return Err(invalid("trailing packet data"));
    }
    Ok(value)
}

pub(crate) fn packet_from<P: PacketId + Serialize>(packet: &P) -> AppResult<UncompressedPacket> {
    Ok(UncompressedPacket::from_packet(packet)?)
}

fn packet_io_error(error: PacketError) -> io::Error {
    match error {
        PacketError::Io(error) | PacketError::VarInt(VarIntError::Io(error)) => error,
        error => io::Error::other(error),
    }
}

pub(crate) struct Connection {
    reader: CryptoStream<OwnedReadHalf>,
    writer: CryptoStream<OwnedWriteHalf>,
}

impl Connection {
    pub(crate) fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: CryptoStream::new(reader),
            writer: CryptoStream::new(writer),
        })
    }

    async fn read(&mut self) -> io::Result<UncompressedPacket> {
        timeout(Duration::from_secs(30), RawPacket::read_async(&mut self.reader))
            .await
            .map_err(|_| io::Error::new(ErrorKind::TimedOut, "packet read timed out"))?
            .and_then(|packet| packet.as_uncompressed())
            .map_err(packet_io_error)
    }

    pub(crate) async fn read_until(&mut self, deadline: Instant) -> io::Result<UncompressedPacket> {
        let mut keepalive = interval(Duration::from_secs(10));
        keepalive.tick().await;
        let packet = RawPacket::read_async(&mut self.reader);
        tokio::pin!(packet);
        loop {
            tokio::select! {
                result = &mut packet => return result.and_then(|packet| packet.as_uncompressed()).map_err(packet_io_error),
                _ = keepalive.tick() => send_packet(&mut self.writer, &ConfigurationKeepAlive { id: 0 }).await?,
                _ = sleep_until(deadline) => return Err(io::Error::new(ErrorKind::TimedOut, "code input timed out")),
            }
        }
    }

    pub(crate) async fn send<P: PacketId + Serialize>(&mut self, packet: &P) -> io::Result<()> {
        send_packet(&mut self.writer, packet).await
    }

    pub(crate) async fn receive<P: Deserialize>(&mut self, id: i32) -> AppResult<P> {
        let packet = self.read().await?;
        if packet.packet_id != id {
            return Err(invalid("unexpected packet ID"));
        }
        decode_exact(&packet.payload)
    }

    pub(crate) fn encrypt(&mut self, key: &[u8; 16]) {
        self.reader.decrypt(key);
        self.writer.encrypt(key);
    }

    pub(crate) async fn close_after_send(&mut self) -> io::Result<()> {
        self.writer.inner.shutdown().await?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buffer = [0_u8; 1024];
        loop {
            match timeout(
                deadline.saturating_duration_since(Instant::now()),
                self.reader.inner.read(&mut buffer),
            )
            .await
            {
                Ok(Ok(0)) | Err(_) => return Ok(()),
                Ok(Ok(_)) => {}
                Ok(Err(error)) if matches!(error.kind(), ErrorKind::ConnectionReset | ErrorKind::TimedOut) => {
                    return Ok(());
                }
                Ok(Err(error)) => return Err(error),
            }
        }
    }
}

async fn send_packet<P: PacketId + Serialize>(writer: &mut CryptoStream<OwnedWriteHalf>, packet: &P) -> io::Result<()> {
    let mut bytes = Vec::new();
    packet_from(packet)
        .map_err(io::Error::other)?
        .write_sync(&mut bytes)
        .map_err(packet_io_error)?;
    writer.write_all(&mut bytes).await
}
