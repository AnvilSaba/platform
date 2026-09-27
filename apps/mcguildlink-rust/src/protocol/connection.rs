use std::{
    io::{self, Cursor, ErrorKind},
    time::Duration,
};

use mc_protocol::{
    packet::{MAX_PACKET_LENGTH, PacketError, PacketId, RawPacket, UncompressedPacket},
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

use super::{
    crypto::{StreamDecryptor, StreamEncryptor},
    packets::ConfigurationKeepAlive,
};
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

struct Reader {
    stream: OwnedReadHalf,
    cipher: Option<StreamDecryptor>,
}

impl Reader {
    async fn read_exact(&mut self, bytes: &mut [u8]) -> io::Result<()> {
        self.stream.read_exact(bytes).await?;
        if let Some(cipher) = &mut self.cipher {
            cipher.apply(bytes);
        }
        Ok(())
    }

    async fn packet(&mut self) -> io::Result<UncompressedPacket> {
        let mut length = 0_usize;
        for shift in [0, 7, 14] {
            let mut byte = [0_u8; 1];
            self.read_exact(&mut byte).await?;
            length |= ((byte[0] & 0x7f) as usize) << shift;
            if byte[0] & 0x80 == 0 {
                if length > MAX_PACKET_LENGTH {
                    return Err(io::Error::new(ErrorKind::InvalidData, "packet too long"));
                }
                let mut data = vec![0_u8; length];
                self.read_exact(&mut data).await?;
                return RawPacket::new(data).as_uncompressed().map_err(packet_io_error);
            }
        }
        Err(io::Error::new(ErrorKind::InvalidData, "packet length VarInt too long"))
    }
}

struct Writer {
    stream: OwnedWriteHalf,
    cipher: Option<StreamEncryptor>,
}

impl Writer {
    async fn send<P: PacketId + Serialize>(&mut self, packet: &P) -> io::Result<()> {
        let mut bytes = Vec::new();
        packet_from(packet)
            .map_err(io::Error::other)?
            .write_sync(&mut bytes)
            .map_err(io::Error::other)?;
        if let Some(cipher) = &mut self.cipher {
            cipher.apply(&mut bytes);
        }
        self.stream.write_all(&bytes).await
    }
}

pub(crate) struct Connection {
    reader: Reader,
    writer: Writer,
}

impl Connection {
    pub(crate) fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: Reader {
                stream: reader,
                cipher: None,
            },
            writer: Writer {
                stream: writer,
                cipher: None,
            },
        })
    }

    async fn read(&mut self) -> io::Result<UncompressedPacket> {
        timeout(Duration::from_secs(30), self.reader.packet())
            .await
            .map_err(|_| io::Error::new(ErrorKind::TimedOut, "packet read timed out"))?
    }

    pub(crate) async fn read_until(&mut self, deadline: Instant) -> io::Result<UncompressedPacket> {
        let mut keepalive = interval(Duration::from_secs(10));
        keepalive.tick().await;
        let packet = self.reader.packet();
        tokio::pin!(packet);
        loop {
            tokio::select! {
                result = &mut packet => return result,
                _ = keepalive.tick() => self.writer.send(&ConfigurationKeepAlive { id: 0 }).await?,
                _ = sleep_until(deadline) => return Err(io::Error::new(ErrorKind::TimedOut, "code input timed out")),
            }
        }
    }

    pub(crate) async fn send<P: PacketId + Serialize>(&mut self, packet: &P) -> io::Result<()> {
        self.writer.send(packet).await
    }

    pub(crate) async fn receive<P: Deserialize>(&mut self, id: i32) -> AppResult<P> {
        let packet = self.read().await?;
        if packet.packet_id != id {
            return Err(invalid("unexpected packet ID"));
        }
        decode_exact(&packet.payload)
    }

    pub(crate) fn encrypt(&mut self, key: &[u8; 16]) {
        self.reader.cipher = Some(StreamDecryptor::new(key));
        self.writer.cipher = Some(StreamEncryptor::new(key));
    }

    pub(crate) async fn close_after_send(&mut self) -> io::Result<()> {
        self.writer.stream.shutdown().await?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buffer = [0_u8; 1024];
        loop {
            match timeout(
                deadline.saturating_duration_since(Instant::now()),
                self.reader.stream.read(&mut buffer),
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
