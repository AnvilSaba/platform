use std::sync::OnceLock;

use mc_protocol::varint::VarInt;
use rsa::{
    Pkcs1v15Encrypt, RsaPrivateKey,
    pkcs8::EncodePublicKey,
    rand_core::{OsRng, RngCore},
};
use tokio::net::TcpStream;
use uuid::Uuid;

use crate::{
    AppResult, VERSION, invalid,
    link_flow::configuration,
    link_store::CodeLinker,
    protocol::{Connection, packets::*},
    session::SessionVerifier,
};

static SESSION_ID: OnceLock<Uuid> = OnceLock::new();

pub(crate) struct LinkServer<V, L> {
    verifier: V,
    linker: L,
}

impl<V: SessionVerifier, L: CodeLinker> LinkServer<V, L> {
    pub(crate) fn new(verifier: V, linker: L) -> Self {
        Self { verifier, linker }
    }

    pub(crate) async fn serve(&self, stream: TcpStream) -> AppResult<()> {
        let mut connection = Connection::new(stream)?;
        let Handshake {
            version,
            host,
            next_state,
            ..
        } = connection.receive::<Handshake>(Handshake::PACKET_ID).await?;
        if host.encode_utf16().count() > 255 {
            return Err(invalid("handshake host too long"));
        }
        match next_state.0 {
            1 => status(&mut connection).await,
            2 if version.0 == VERSION => self.login(&mut connection).await,
            2 => {
                connection
                    .send(&LoginDisconnect {
                        reason: TextComponent::new("Minecraft Java 26.3 を使用してください。"),
                    })
                    .await?;
                connection.close_after_send().await?;
                Err(invalid("unsupported Minecraft protocol version"))
            }
            _ => Err(invalid("unsupported next state")),
        }
    }

    async fn login(&self, connection: &mut Connection) -> AppResult<()> {
        let LoginHello { name, uuid } = connection.receive::<LoginHello>(LoginHello::PACKET_ID).await?;

        let rsa = RsaPrivateKey::new(&mut OsRng, 1024)?;
        let public_key = rsa.to_public_key().to_public_key_der()?.as_bytes().to_vec();
        let mut challenge = [0_u8; 16];
        OsRng.fill_bytes(&mut challenge);
        connection
            .send(&EncryptionRequest {
                server_id: String::new(),
                public_key: public_key.clone(),
                challenge: challenge.to_vec(),
                authenticate: true,
            })
            .await?;

        let EncryptionResponse {
            secret: encrypted_secret,
            challenge: encrypted_challenge,
        } = connection
            .receive::<EncryptionResponse>(EncryptionResponse::PACKET_ID)
            .await?;
        if encrypted_secret.len() > 512 || encrypted_challenge.len() > 512 {
            return Err(invalid("encrypted field too long"));
        }
        let secret = rsa.decrypt(Pkcs1v15Encrypt, &encrypted_secret)?;
        let supplied_challenge = rsa.decrypt(Pkcs1v15Encrypt, &encrypted_challenge)?;
        if secret.len() != 16 || supplied_challenge != challenge {
            return Err(invalid("encryption challenge failed"));
        }
        let key: [u8; 16] = secret[..16].try_into().unwrap();
        connection.encrypt(&key);

        let profile = self.verifier.authenticate(name.as_ref(), &key, &public_key).await?;
        if !profile.name.as_ref().eq_ignore_ascii_case(name.as_ref()) {
            return Err(invalid("session profile name mismatch"));
        }
        if uuid != profile.id {
            return Err(invalid("session profile UUID mismatch"));
        }

        connection
            .send(&LoginFinished {
                uuid: profile.id,
                name: profile.name.as_ref().to_owned(),
                properties: VarInt(0),
                session_id: *SESSION_ID.get_or_init(Uuid::new_v4),
            })
            .await?;
        connection
            .receive::<LoginAcknowledged>(LoginAcknowledged::PACKET_ID)
            .await?;
        eprintln!(
            "Authenticated Minecraft player {} ({})",
            profile.name.as_ref(),
            profile.id
        );
        configuration(connection, &profile, &self.linker).await
    }
}

async fn status(connection: &mut Connection) -> AppResult<()> {
    connection.receive::<StatusRequest>(StatusRequest::PACKET_ID).await?;
    let status = Status::new(
        StatusVersion::new("26.3", VERSION),
        StatusPlayers::new(128, 0),
        TextComponent::new("アカウント紐付け用サーバー"),
    );
    connection.send(&StatusResponse { status }).await?;
    if let Ok(ping) = connection.receive::<StatusPing>(StatusPing::PACKET_ID).await {
        connection
            .send(&StatusPong {
                timestamp: ping.timestamp,
            })
            .await?;
    }
    Ok(())
}
