use std::io::{Read, Write};

use mc_protocol::{
    Packet,
    ser::{Deserialize, SerializationError, Serialize},
    varint::VarInt,
};
use uuid::Uuid;

pub struct RemainingBytes(pub Vec<u8>);

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
pub struct Handshake {
    pub version: VarInt,
    pub host: String,
    pub port: u16,
    pub next_state: VarInt,
}

#[derive(Packet)]
#[packet(0)]
pub struct StatusRequest {}

#[derive(Packet)]
#[packet(0)]
pub struct StatusResponse {
    pub json: String,
}

#[derive(Packet)]
#[packet(1)]
pub struct StatusPing {
    pub timestamp: i64,
}

#[derive(Packet)]
#[packet(1)]
pub struct StatusPong {
    pub timestamp: i64,
}

#[derive(Packet)]
#[packet(0)]
pub struct LoginHello {
    pub name: String,
    pub uuid: Uuid,
}

#[derive(Packet)]
#[packet(0)]
pub struct LoginDisconnect {
    pub reason_json: String,
}

#[derive(Packet)]
#[packet(1)]
pub struct EncryptionRequest {
    pub server_id: String,
    pub public_key: Vec<u8>,
    pub challenge: Vec<u8>,
    pub authenticate: bool,
}

#[derive(Packet)]
#[packet(1)]
pub struct EncryptionResponse {
    pub secret: Vec<u8>,
    pub challenge: Vec<u8>,
}

#[derive(Packet)]
#[packet(2)]
pub struct LoginFinished {
    pub uuid: Uuid,
    pub name: String,
    pub properties: VarInt,
    pub session_id: Uuid,
}

#[derive(Packet)]
#[packet(3)]
pub struct LoginAcknowledged {}

#[derive(Packet)]
#[packet(19)]
pub struct ConfigurationDialog {
    pub document: RemainingBytes,
}

#[derive(Packet)]
#[packet(8)]
pub struct ConfigurationDialogClick {
    pub action: String,
    pub data: Vec<u8>,
}

#[derive(Packet)]
#[packet(4)]
pub struct ConfigurationKeepalive {
    pub id: i64,
}

#[derive(Packet)]
#[packet(4)]
pub struct ConfigurationKeepaliveResponse {
    pub id: i64,
}

#[derive(Packet)]
#[packet(2)]
pub struct ConfigurationDisconnect {
    pub reason: RemainingBytes,
}

#[derive(Packet)]
#[packet(3)]
pub struct ConfigurationFinishAcknowledged {}

#[derive(Packet)]
#[packet(0)]
pub struct ConfigurationClientInformation {
    pub data: RemainingBytes,
}

#[derive(Packet)]
#[packet(2)]
pub struct ConfigurationCustomPayload {
    pub data: RemainingBytes,
}

#[derive(Packet)]
#[packet(5)]
pub struct ConfigurationPong {
    pub data: RemainingBytes,
}

#[derive(Packet)]
#[packet(6)]
pub struct ConfigurationResourcePackResponse {
    pub data: RemainingBytes,
}

#[derive(Packet)]
#[packet(7)]
pub struct ConfigurationKnownPacks {
    pub data: RemainingBytes,
}
