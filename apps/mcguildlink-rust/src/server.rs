use std::{net::TcpStream, sync::OnceLock};

use mc_protocol::varint::VarInt;
use rand::{RngCore, rngs::OsRng};
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, pkcs8::EncodePublicKey};
use uuid::Uuid;

use crate::{
    AppResult, VERSION, invalid,
    link_flow::configuration,
    protocol::{Connection, packets::*},
    session::{SessionProfile, authenticate},
};

static SESSION_ID: OnceLock<Uuid> = OnceLock::new();

pub(crate) struct VerifiedPlayer {
    pub(crate) uuid: Uuid,
    pub(crate) name: String,
}

pub fn serve(stream: TcpStream) -> AppResult<()> {
    serve_with_verifier(stream, authenticate)
}

pub(crate) fn serve_with_verifier<F>(stream: TcpStream, verifier: F) -> AppResult<()>
where
    F: Fn(&str, &[u8; 16], &[u8]) -> AppResult<SessionProfile>,
{
    let mut connection = Connection::new(stream)?;
    let Handshake {
        version,
        host,
        next_state,
        ..
    } = connection.receive::<Handshake>(Handshake::PACKET_ID)?;
    if host.encode_utf16().count() > 255 {
        return Err(invalid("handshake host too long"));
    }
    match next_state.0 {
        1 => status(&mut connection),
        2 if version.0 == VERSION => login(&mut connection, &verifier),
        2 => {
            connection.send(&LoginDisconnect {
                reason_json: serde_json::json!({ "text": "Minecraft Java 26.3 を使用してください。" }).to_string(),
            })?;
            connection.close_after_send()?;
            Err(invalid("unsupported Minecraft protocol version"))
        }
        _ => Err(invalid("unsupported next state")),
    }
}

fn login<F>(connection: &mut Connection, verifier: &F) -> AppResult<()>
where
    F: Fn(&str, &[u8; 16], &[u8]) -> AppResult<SessionProfile>,
{
    let LoginHello {
        name: requested_name,
        uuid: requested_uuid,
    } = connection.receive::<LoginHello>(LoginHello::PACKET_ID)?;
    if requested_name.len() > 16 || requested_name.is_empty() {
        return Err(invalid("invalid player name"));
    }
    let rsa = RsaPrivateKey::new(&mut OsRng, 1024)?;
    let public_key = rsa.to_public_key().to_public_key_der()?.as_bytes().to_vec();
    let mut challenge = [0_u8; 16];
    OsRng.fill_bytes(&mut challenge);
    connection.send(&EncryptionRequest {
        server_id: String::new(),
        public_key: public_key.clone(),
        challenge: challenge.to_vec(),
        authenticate: true,
    })?;

    let EncryptionResponse {
        secret: encrypted_secret,
        challenge: encrypted_challenge,
    } = connection.receive::<EncryptionResponse>(EncryptionResponse::PACKET_ID)?;
    if encrypted_secret.len() > 512 || encrypted_challenge.len() > 512 {
        return Err(invalid("encrypted field too long"));
    }
    let secret = decrypt(&rsa, &encrypted_secret)?;
    let supplied_challenge = decrypt(&rsa, &encrypted_challenge)?;
    if secret.len() != 16 || supplied_challenge != challenge {
        return Err(invalid("encryption challenge failed"));
    }
    let mut key = [0_u8; 16];
    key.copy_from_slice(&secret);
    connection.encrypt(&key)?;

    let profile = verifier(&requested_name, &key, &public_key)?;
    let id = Uuid::parse_str(&profile.id)?;
    if !profile.name.eq_ignore_ascii_case(&requested_name) {
        return Err(invalid("session profile name mismatch"));
    }
    if requested_uuid != id {
        return Err(invalid("session profile UUID mismatch"));
    }

    connection.send(&LoginFinished {
        uuid: id,
        name: profile.name.clone(),
        properties: VarInt(0),
        session_id: *SESSION_ID.get_or_init(Uuid::new_v4),
    })?;
    connection.receive::<LoginAcknowledged>(LoginAcknowledged::PACKET_ID)?;
    let player = VerifiedPlayer {
        uuid: id,
        name: profile.name,
    };
    eprintln!("Authenticated Minecraft player {} ({})", player.name, player.uuid);
    configuration(connection, &player)
}

fn decrypt(rsa: &RsaPrivateKey, ciphertext: &[u8]) -> AppResult<Vec<u8>> {
    Ok(rsa.decrypt(Pkcs1v15Encrypt, ciphertext)?)
}

fn status(connection: &mut Connection) -> AppResult<()> {
    connection.receive::<StatusRequest>(StatusRequest::PACKET_ID)?;
    let response = serde_json::json!({
        "version": { "name": "26.3", "protocol": VERSION },
        "players": { "max": 0, "online": 0 },
        "description": { "text": "アカウント紐付け用サーバー" }
    });
    connection.send(&StatusResponse {
        json: response.to_string(),
    })?;
    if let Ok(ping) = connection.receive::<StatusPing>(StatusPing::PACKET_ID) {
        connection.send(&StatusPong {
            timestamp: ping.timestamp,
        })?;
    }
    Ok(())
}
