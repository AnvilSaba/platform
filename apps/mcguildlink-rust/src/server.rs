use std::{net::TcpStream, sync::OnceLock};

use mc_protocol::varint::VarInt;
use rsa::{
    Pkcs1v15Encrypt, RsaPrivateKey,
    pkcs8::EncodePublicKey,
    rand_core::{OsRng, RngCore},
};
use uuid::Uuid;

use crate::{
    AppResult, VERSION, invalid,
    link_flow::configuration,
    protocol::{Connection, packets::*},
    session::{SessionProfile, authenticate},
};

static SESSION_ID: OnceLock<Uuid> = OnceLock::new();

pub(crate) fn serve(stream: TcpStream) -> AppResult<()> {
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
                reason: TextComponent::new("Minecraft Java 26.3 を使用してください。"),
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
    let LoginHello { name, uuid } = connection.receive::<LoginHello>(LoginHello::PACKET_ID)?;

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
    let key = secret[..16].try_into().unwrap();
    connection.encrypt(key)?;

    let profile = verifier(name.as_ref(), key, &public_key)?;
    let id = profile.id;
    if !profile.name.as_ref().eq_ignore_ascii_case(name.as_ref()) {
        return Err(invalid("session profile name mismatch"));
    }
    if uuid != id {
        return Err(invalid("session profile UUID mismatch"));
    }

    connection.send(&LoginFinished {
        uuid: id,
        name: profile.name.as_ref().to_owned(),
        properties: VarInt(0),
        session_id: *SESSION_ID.get_or_init(Uuid::new_v4),
    })?;
    connection.receive::<LoginAcknowledged>(LoginAcknowledged::PACKET_ID)?;
    eprintln!(
        "Authenticated Minecraft player {} ({})",
        profile.name.as_ref(),
        profile.id
    );
    configuration(connection, &profile)
}

fn decrypt(rsa: &RsaPrivateKey, ciphertext: &[u8]) -> AppResult<Vec<u8>> {
    Ok(rsa.decrypt(Pkcs1v15Encrypt, ciphertext)?)
}

fn status(connection: &mut Connection) -> AppResult<()> {
    connection.receive::<StatusRequest>(StatusRequest::PACKET_ID)?;
    let status = Status::new(
        StatusVersion::new("26.3", VERSION),
        StatusPlayers::new(128, 0),
        TextComponent::new("アカウント紐付け用サーバー"),
    );
    connection.send(&StatusResponse { status })?;
    if let Ok(ping) = connection.receive::<StatusPing>(StatusPing::PACKET_ID) {
        connection.send(&StatusPong {
            timestamp: ping.timestamp,
        })?;
    }
    Ok(())
}
