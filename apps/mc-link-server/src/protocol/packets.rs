use std::{
    borrow::Cow,
    io::{Error as StdIOError, Read, Write},
};

use fastnbt::{DeOpts, SerOpts};
use mc_protocol::{
    Packet,
    ser::{Deserialize, SerializationError, Serialize},
    varint::VarInt,
};
use uuid::Uuid;

use crate::identity::Name;

impl Serialize for Name {
    fn serialize<W: Write + Unpin>(&self, writer: &mut W) -> Result<(), SerializationError> {
        self.as_ref().serialize(writer)
    }
}

impl Deserialize for Name {
    fn deserialize<R: Read + Unpin>(reader: &mut R) -> Result<Self, SerializationError> {
        Ok(Name::try_new(&String::deserialize(reader)?).map_err(StdIOError::other)?)
    }
}

#[derive(Debug)]
struct RemainingBytes(Vec<u8>);

impl Serialize for RemainingBytes {
    fn serialize<W: Write + Unpin>(&self, writer: &mut W) -> Result<(), SerializationError> {
        writer.write_all(&self.0)?;
        Ok(())
    }
}

impl Deserialize for RemainingBytes {
    fn deserialize<R: Read + Unpin>(reader: &mut R) -> Result<Self, SerializationError> {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        Ok(Self(data))
    }
}

#[derive(Packet)]
#[packet(0)]
pub(crate) struct Handshake {
    pub(crate) version: VarInt,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) next_state: VarInt,
}

#[derive(Packet)]
#[packet(0)]
pub(crate) struct StatusRequest {}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct TextComponent<'a> {
    pub(crate) text: Cow<'a, str>,
}

impl<'a> TextComponent<'a> {
    pub(crate) fn new(text: impl Into<Cow<'a, str>>) -> Self {
        Self { text: text.into() }
    }
}

impl Serialize for TextComponent<'_> {
    fn serialize<W: Write + Unpin>(&self, writer: &mut W) -> Result<(), SerializationError> {
        serde_json::to_string(&self)
            .map_err(StdIOError::other)?
            .serialize(writer)
    }
}

impl Deserialize for TextComponent<'_> {
    fn deserialize<R: Read + Unpin>(reader: &mut R) -> Result<Self, SerializationError> {
        Ok(serde_json::from_str::<Self>(&String::deserialize(reader)?).map_err(StdIOError::other)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct NbtTextComponent<'a> {
    pub(crate) text: Cow<'a, str>,
}

impl<'a> NbtTextComponent<'a> {
    pub(crate) fn new(text: impl Into<Cow<'a, str>>) -> Self {
        Self { text: text.into() }
    }
}

impl Serialize for NbtTextComponent<'_> {
    fn serialize<W: Write + Unpin>(&self, writer: &mut W) -> Result<(), SerializationError> {
        Ok(fastnbt::to_writer_with_opts(writer, &self, SerOpts::network_nbt()).map_err(StdIOError::other)?)
    }
}

impl Deserialize for NbtTextComponent<'_> {
    fn deserialize<R: Read + Unpin>(reader: &mut R) -> Result<Self, SerializationError> {
        Ok(fastnbt::from_reader_with_opts(reader, DeOpts::network_nbt()).map_err(StdIOError::other)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct StatusVersion<'a> {
    name: Cow<'a, str>,
    protocol: i32,
}

impl<'a> StatusVersion<'a> {
    pub(crate) fn new(name: impl Into<Cow<'a, str>>, protocol: i32) -> Self {
        Self {
            name: name.into(),
            protocol,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct StatusPlayers {
    max: i32,
    online: i32,
}

impl StatusPlayers {
    pub(crate) fn new(max: i32, online: i32) -> Self {
        Self { max, online }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Status<'a> {
    version: StatusVersion<'a>,
    players: StatusPlayers,
    description: TextComponent<'a>,
}

impl<'a> Status<'a> {
    pub(crate) fn new(version: StatusVersion<'a>, players: StatusPlayers, description: TextComponent<'a>) -> Self {
        Self {
            version,
            players,
            description,
        }
    }

    #[cfg(test)]
    pub(crate) fn version_name(&self) -> &str {
        &self.version.name
    }

    #[cfg(test)]
    pub(crate) fn version_protocol(&self) -> i32 {
        self.version.protocol
    }
}

impl Serialize for Status<'_> {
    fn serialize<W: Write + Unpin>(&self, writer: &mut W) -> Result<(), SerializationError> {
        serde_json::to_string(&self)
            .map_err(StdIOError::other)?
            .serialize(writer)
    }
}

impl Deserialize for Status<'_> {
    fn deserialize<R: Read + Unpin>(reader: &mut R) -> Result<Self, SerializationError> {
        Ok(serde_json::from_str::<Self>(&String::deserialize(reader)?).map_err(StdIOError::other)?)
    }
}

#[derive(Debug, Packet)]
#[packet(0)]
pub(crate) struct StatusResponse<'a> {
    pub(crate) status: Status<'a>,
}

#[derive(Debug, Packet)]
#[packet(1)]
pub(crate) struct StatusPing {
    pub(crate) timestamp: i64,
}

#[derive(Debug, Packet)]
#[packet(1)]
pub(crate) struct StatusPong {
    pub(crate) timestamp: i64,
}

#[derive(Debug, Packet)]
#[packet(0)]
pub(crate) struct LoginHello {
    pub(crate) name: Name,
    pub(crate) uuid: Uuid,
}

#[derive(Debug, Packet)]
#[packet(0)]
pub(crate) struct LoginDisconnect<'a> {
    pub(crate) reason: TextComponent<'a>,
}

#[derive(Debug, Packet)]
#[packet(1)]
pub(crate) struct EncryptionRequest {
    pub(crate) server_id: String,
    pub(crate) public_key: Vec<u8>,
    pub(crate) challenge: Vec<u8>,
    pub(crate) authenticate: bool,
}

#[derive(Debug, Packet)]
#[packet(1)]
pub(crate) struct EncryptionResponse {
    pub(crate) secret: Vec<u8>,
    pub(crate) challenge: Vec<u8>,
}

#[derive(Debug, Packet)]
#[packet(2)]
pub(crate) struct LoginFinished {
    pub(crate) uuid: Uuid,
    pub(crate) name: String,
    pub(crate) properties: VarInt,
    pub(crate) session_id: Uuid,
}

#[derive(Debug, Packet)]
#[packet(3)]
pub(crate) struct LoginAcknowledged {}

#[derive(Debug, Packet)]
#[packet(19)]
pub(crate) struct ConfigurationDialog<'a> {
    pub(crate) document: super::dialog::DialogDocument<'a>,
}

#[derive(Debug, Packet)]
#[packet(8)]
pub(crate) struct ConfigurationDialogClick<D> {
    pub(crate) action: String,
    pub(crate) data: D,
}

#[derive(Debug, Packet)]
#[packet(4)]
pub(crate) struct ConfigurationKeepAlive {
    pub(crate) id: i64,
}

#[derive(Debug, Packet)]
#[packet(4)]
pub(crate) struct ConfigurationKeepAliveResponse {
    pub(crate) id: i64,
}

#[derive(Debug, Packet)]
#[packet(2)]
pub(crate) struct ConfigurationDisconnect<'a> {
    pub(crate) reason: NbtTextComponent<'a>,
}

#[derive(Debug, Packet)]
#[packet(3)]
pub(crate) struct ConfigurationFinishAcknowledged {}

#[derive(Debug, Packet)]
#[packet(0)]
pub(crate) struct ConfigurationClientInformation {
    data: RemainingBytes,
}

#[derive(Debug, Packet)]
#[packet(2)]
pub(crate) struct ConfigurationCustomPayload {
    data: RemainingBytes,
}

#[derive(Debug, Packet)]
#[packet(5)]
pub(crate) struct ConfigurationPong {
    data: RemainingBytes,
}

#[derive(Debug, Packet)]
#[packet(6)]
pub(crate) struct ConfigurationResourcePackResponse {
    data: RemainingBytes,
}

#[derive(Debug, Packet)]
#[packet(7)]
pub(crate) struct ConfigurationKnownPacks {
    data: RemainingBytes,
}
