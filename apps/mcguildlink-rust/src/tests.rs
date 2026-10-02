use crate::{
    VERSION,
    identity::Name,
    link_flow::configuration,
    link_store::{CodeLinker, LinkResult},
    protocol::{
        Connection,
        connection::{decode_exact, packet_from},
        crypto::{Cfb8Reader, Cfb8Writer},
        dialog::{self, Submission},
        packets::*,
    },
    server::LinkServer,
    session::{SessionProfile, SessionVerifier, signed_sha1},
};
use mc_protocol::{
    packet::{PacketId, RawPacket, UncompressedPacket},
    ser::Serialize,
    varint::VarInt,
};
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey, rand_core::OsRng};
use std::{io::Write, net::TcpStream, time::Duration};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread")]
async fn status_advertises_26_3_and_echoes_ping() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { test_server().serve(listener.accept().await.unwrap().0).await });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    send_packet(
        &mut client,
        &Handshake {
            version: VarInt(VERSION),
            host: "localhost".into(),
            port: 25565,
            next_state: VarInt(1),
        },
    );
    send_packet(&mut client, &StatusRequest {});
    let reply = RawPacket::read_sync(&mut client).unwrap().as_uncompressed().unwrap();
    assert_eq!(reply.packet_id, 0);
    let status: StatusResponse = decode_exact(&reply.payload).unwrap();
    assert_eq!(status.status.version_protocol(), 777);
    assert_eq!(status.status.version_name(), "26.3");
    send_packet(&mut client, &StatusPing { timestamp: 123 });
    let pong = RawPacket::read_sync(&mut client).unwrap().as_uncompressed().unwrap();
    assert_eq!(pong.packet_id, 1);
    assert_eq!(decode_exact::<StatusPong>(&pong.payload).unwrap().timestamp, 123);
    assert!(server.await.unwrap().is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn older_protocol_never_reaches_login() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { test_server().serve(listener.accept().await.unwrap().0).await });
    let mut client = TcpStream::connect(address).unwrap();
    send_packet(
        &mut client,
        &Handshake {
            version: VarInt(VERSION - 1),
            host: "localhost".into(),
            port: 25565,
            next_state: VarInt(2),
        },
    );
    send_packet(
        &mut client,
        &LoginHello {
            name: Name::try_new("TestPlayer").unwrap(),
            uuid: Uuid::nil(),
        },
    );
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let disconnect = read_packet(&mut client);
    assert_eq!(disconnect.packet_id, LoginDisconnect::PACKET_ID);
    let reason: LoginDisconnect<'_> = decode_exact(&disconnect.payload).unwrap();
    assert!(reason.reason.text.contains("26.3"));
    drop(client);
    assert!(server.await.unwrap().is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn configuration_retries_invalid_code_then_shows_result_and_disconnects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut connection = Connection::new(listener.accept().await.unwrap().0).unwrap();
        configuration(&mut connection, &test_player(), Duration::from_secs(2), &TestLinker).await
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let first_dialog = read_packet(&mut client);
    assert_eq!(first_dialog.packet_id, 19);
    assert_eq!(
        first_dialog.payload.first(),
        Some(&10),
        "dialog NBT must begin with compound tag"
    );
    send_click(&mut client, dialog::SUBMIT_ID, Some("WRONG123"));
    let retry = read_packet(&mut client);
    assert_eq!(retry.packet_id, 19);
    let _: fastnbt::Value = fastnbt::from_bytes_with_opts(&retry.payload, fastnbt::DeOpts::network_nbt()).unwrap();
    assert!(
        retry
            .payload
            .windows("無効なコード".len())
            .any(|bytes| bytes == "無効なコード".as_bytes())
    );
    send_click(&mut client, dialog::SUBMIT_ID, Some("SUCCESS1"));
    let result = read_packet(&mut client);
    assert_eq!(result.packet_id, 19);
    let _: fastnbt::Value = fastnbt::from_bytes_with_opts(&result.payload, fastnbt::DeOpts::network_nbt()).unwrap();
    assert!(
        result
            .payload
            .windows("成功しました。".len())
            .any(|bytes| bytes == "成功しました。".as_bytes())
    );
    send_click(&mut client, dialog::DONE_ID, None);
    let disconnect = read_packet(&mut client);
    assert_eq!(disconnect.packet_id, 2);
    let reason: NbtTextComponent<'_> = decode_exact(&disconnect.payload).unwrap();
    assert_eq!(reason.text, "正常に切断されました。");
    assert!(server.await.unwrap().is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn configuration_times_out_with_disconnect_reason() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut connection = Connection::new(listener.accept().await.unwrap().0).unwrap();
        configuration(&mut connection, &test_player(), Duration::from_millis(100), &TestLinker).await
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    assert_eq!(read_packet(&mut client).packet_id, 19);
    let disconnect = read_packet(&mut client);
    assert_eq!(disconnect.packet_id, 2);
    let reason: NbtTextComponent<'_> = decode_exact(&disconnect.payload).unwrap();
    assert!(reason.text.contains("コードを入力する時間"));
    assert!(server.await.unwrap().is_ok());
}

#[test]
fn session_hash_uses_signed_twos_complement() {
    assert_eq!(signed_sha1(b"abc"), "-5666c1c9b8f97e9545c1da8e87af3d93632f2763");
}

#[test]
fn player_name_is_validated_in_login_and_session_profile() {
    for invalid in ["", "Name-With-Dash", "Name With Space", "日本語", "abcdefghijklmnopq"] {
        assert!(Name::try_new(invalid).is_err(), "accepted invalid name: {invalid}");
        let json = serde_json::json!({ "id": Uuid::nil(), "name": invalid });
        assert!(serde_json::from_value::<SessionProfile>(json).is_err());
    }
    assert!(Name::try_new("A_b1").is_ok());
    // Existing accounts can have names shorter than today's creation minimum.
    assert!(Name::try_new("a").is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn authenticated_login_enters_configuration_without_play() {
    let player_id = Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { test_server().serve(listener.accept().await.unwrap().0).await });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    send_packet(
        &mut client,
        &Handshake {
            version: VarInt(VERSION),
            host: "localhost".into(),
            port: 25565,
            next_state: VarInt(2),
        },
    );
    send_packet(
        &mut client,
        &LoginHello {
            name: Name::try_new("TestPlayer").unwrap(),
            uuid: player_id,
        },
    );

    let request = read_packet(&mut client);
    assert_eq!(request.packet_id, 1);
    let EncryptionRequest {
        server_id,
        public_key,
        challenge,
        authenticate,
    } = decode_exact(&request.payload).unwrap();
    assert_eq!(server_id, "");
    assert!(authenticate);
    let rsa = RsaPublicKey::from_public_key_der(&public_key).unwrap();
    let secret = [0x42_u8; 16];
    let encrypted_secret = rsa.encrypt(&mut OsRng, Pkcs1v15Encrypt, &secret).unwrap();
    let encrypted_challenge = rsa.encrypt(&mut OsRng, Pkcs1v15Encrypt, &challenge).unwrap();
    let response = EncryptionResponse {
        secret: encrypted_secret,
        challenge: encrypted_challenge,
    };
    send_packet(&mut client, &response);

    let mut reader = Cfb8Reader::new(client.try_clone().unwrap(), &secret);
    let mut writer = Cfb8Writer::new(client.try_clone().unwrap(), &secret);
    let finished = RawPacket::read_sync(&mut reader).unwrap().as_uncompressed().unwrap();
    assert_eq!(finished.packet_id, 2);
    let finished: LoginFinished = decode_exact(&finished.payload).unwrap();
    assert_eq!(finished.uuid, player_id);
    assert_eq!(finished.name, "TestPlayer");
    assert_eq!(finished.properties.0, 0);
    send_packet(&mut writer, &LoginAcknowledged {});
    let dialog = RawPacket::read_sync(&mut reader).unwrap().as_uncompressed().unwrap();
    assert_eq!(dialog.packet_id, 19);
    send_click(&mut writer, dialog::SUBMIT_ID, Some("SUCCESS1"));
    assert_eq!(
        RawPacket::read_sync(&mut reader)
            .unwrap()
            .as_uncompressed()
            .unwrap()
            .packet_id,
        19
    );
    send_click(&mut writer, dialog::DONE_ID, None);
    assert_eq!(
        RawPacket::read_sync(&mut reader)
            .unwrap()
            .as_uncompressed()
            .unwrap()
            .packet_id,
        2
    );
    assert!(server.await.unwrap().is_ok());
}

fn read_packet(stream: &mut TcpStream) -> UncompressedPacket {
    RawPacket::read_sync(stream).unwrap().as_uncompressed().unwrap()
}

fn send_packet<W: Write, P: PacketId + Serialize>(stream: &mut W, packet: &P) {
    packet_from(packet).unwrap().write_sync(stream).unwrap();
}

fn send_click<W: Write>(stream: &mut W, id: &str, code: Option<&str>) {
    send_packet(
        stream,
        &ConfigurationDialogClick {
            action: id.into(),
            data: Submission {
                code: code.map(str::to_string),
            },
        },
    );
}

fn test_player() -> SessionProfile {
    SessionProfile {
        id: Uuid::nil(),
        name: Name::try_new("TestPlayer").unwrap(),
    }
}

struct TestLinker;

impl CodeLinker for TestLinker {
    async fn consume(&self, code: &str, _: &SessionProfile) -> crate::AppResult<LinkResult> {
        Ok(match code {
            "SUCCESS1" => LinkResult::Success("検証用ユーザー".into()),
            "ALREADY1" => LinkResult::AlreadyLinked,
            "BLOCKED1" => LinkResult::Blocked,
            _ => LinkResult::InvalidCode,
        })
    }
}

struct TestVerifier;

impl SessionVerifier for TestVerifier {
    async fn authenticate(&self, name: &str, _: &[u8; 16], _: &[u8]) -> crate::AppResult<SessionProfile> {
        assert_eq!(name, "TestPlayer");
        Ok(SessionProfile {
            id: Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap(),
            name: Name::try_new("TestPlayer").unwrap(),
        })
    }
}

fn test_server() -> LinkServer<TestVerifier, TestLinker> {
    LinkServer::new(TestVerifier, TestLinker)
}
