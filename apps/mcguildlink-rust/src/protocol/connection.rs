use std::{
    io::{self, Cursor, ErrorKind, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

use enum_dispatch::enum_dispatch;
use mc_protocol::{
    packet::{PacketError, PacketId, RawPacket, UncompressedPacket},
    ser::{Deserialize, Serialize},
    varint::VarIntError,
};

use super::{
    crypto::{Cfb8Reader, Cfb8Writer},
    packets::ConfigurationKeepalive,
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

pub(crate) struct Connection {
    stream: TcpStream,
    reader: Reader,
    writer: Box<dyn Write + Send>,
}

#[enum_dispatch(ConnectionReader)]
enum Reader {
    Plain(TcpStream),
    Encrypted(Box<Cfb8Reader<TcpStream>>),
}

#[enum_dispatch]
trait ConnectionReader {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize>;
}

impl ConnectionReader for TcpStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }

    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        Read::read(self, buffer)
    }
}

impl ConnectionReader for Box<Cfb8Reader<TcpStream>> {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        Cfb8Reader::set_read_timeout(self, timeout)
    }

    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        Read::read(self, buffer)
    }
}

impl Read for Reader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        ConnectionReader::read(self, buffer)
    }
}

struct KeepaliveReader<'a> {
    reader: &'a mut Reader,
    writer: &'a mut (dyn Write + Send),
    deadline: Instant,
}

impl Read for KeepaliveReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(ErrorKind::TimedOut, "code input timed out"));
            }
            self.reader
                .set_read_timeout(Some(remaining.min(Duration::from_secs(10))))?;
            match Read::read(self.reader, buffer) {
                Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    if remaining > Duration::from_secs(10) {
                        packet_from(&ConfigurationKeepalive { id: 0 })
                            .map_err(io::Error::other)?
                            .write_sync(&mut self.writer)
                            .map_err(io::Error::other)?;
                        self.writer.flush()?;
                    }
                }
                result => return result,
            }
        }
    }
}

impl Connection {
    pub(crate) fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        reader.set_read_timeout(Some(Duration::from_secs(30)))?;
        Ok(Self {
            reader: Reader::Plain(reader),
            writer: Box::new(stream.try_clone()?),
            stream,
        })
    }

    fn read(&mut self) -> io::Result<UncompressedPacket> {
        RawPacket::read_sync(&mut self.reader)
            .and_then(|packet| packet.as_uncompressed())
            .map_err(packet_io_error)
    }

    pub(crate) fn read_until(&mut self, deadline: Instant) -> io::Result<UncompressedPacket> {
        let mut reader = KeepaliveReader {
            reader: &mut self.reader,
            writer: &mut *self.writer,
            deadline,
        };
        RawPacket::read_sync(&mut reader)
            .and_then(|packet| packet.as_uncompressed())
            .map_err(packet_io_error)
    }

    pub(crate) fn send<P: PacketId + Serialize>(&mut self, packet: &P) -> io::Result<()> {
        packet_from(packet)
            .map_err(io::Error::other)?
            .write_sync(&mut self.writer)
            .map_err(io::Error::other)?;
        self.writer.flush()
    }

    pub(crate) fn receive<P: Deserialize>(&mut self, id: i32) -> AppResult<P> {
        let packet = self.read()?;
        if packet.packet_id != id {
            return Err(invalid("unexpected packet ID"));
        }
        decode_exact(&packet.payload)
    }

    pub(crate) fn encrypt(&mut self, key: &[u8; 16]) -> io::Result<()> {
        let reader = self.stream.try_clone()?;
        reader.set_read_timeout(Some(Duration::from_secs(30)))?;
        self.reader = Reader::Encrypted(Box::new(Cfb8Reader::new(reader, key)));
        self.writer = Box::new(Cfb8Writer::new(self.stream.try_clone()?, key));
        Ok(())
    }
}
