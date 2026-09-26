use crate::{
    VERSION,
    link_flow::configuration_with_timeout,
    protocol::{
        Connection,
        connection::{decode_exact, packet_from},
        crypto::{Cfb8Reader, Cfb8Writer},
        dialog,
        packets::*,
    },
    server::{VerifiedPlayer, serve, serve_with_verifier},
    session::{SessionProfile, signed_sha1},
};
use mc_protocol::{
    packet::{PacketId, RawPacket, UncompressedPacket},
    ser::Serialize,
    varint::VarInt,
};
use rand::rngs::OsRng;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    time::Duration,
};
use uuid::Uuid;

#[test]
fn status_advertises_26_3_and_echoes_ping() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || serve(listener.accept().unwrap().0));
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
    assert!(status.json.contains("\"protocol\":777"));
    assert!(status.json.contains("\"name\":\"26.3\""));
    send_packet(&mut client, &StatusPing { timestamp: 123 });
    let pong = RawPacket::read_sync(&mut client).unwrap().as_uncompressed().unwrap();
    assert_eq!(pong.packet_id, 1);
    assert_eq!(decode_exact::<StatusPong>(&pong.payload).unwrap().timestamp, 123);
    assert!(server.join().unwrap().is_ok());
}

#[test]
fn older_protocol_never_reaches_login() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || serve(listener.accept().unwrap().0));
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
    assert!(server.join().unwrap().is_err());
}

#[test]
fn configuration_retries_invalid_code_then_shows_result_and_disconnects() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut connection = Connection::new(listener.accept().unwrap().0).unwrap();
        configuration_with_timeout(&mut connection, &test_player(), Duration::from_secs(2))
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    assert_eq!(read_packet(&mut client).packet_id, 19);
    send_click(&mut client, dialog::SUBMIT_ID, Some("WRONG123"));
    let retry = read_packet(&mut client);
    assert_eq!(retry.packet_id, 19);
    assert!(
        retry
            .payload
            .windows("無効なコード".len())
            .any(|bytes| bytes == "無効なコード".as_bytes())
    );
    send_click(&mut client, dialog::SUBMIT_ID, Some("SUCCESS1"));
    let result = read_packet(&mut client);
    assert_eq!(result.packet_id, 19);
    assert!(
        result
            .payload
            .windows("成功しました。".len())
            .any(|bytes| bytes == "成功しました。".as_bytes())
    );
    send_click(&mut client, dialog::DONE_ID, None);
    assert_eq!(read_packet(&mut client).packet_id, 2);
    assert!(server.join().unwrap().is_ok());
}

#[test]
fn configuration_times_out_with_disconnect_reason() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut connection = Connection::new(listener.accept().unwrap().0).unwrap();
        configuration_with_timeout(&mut connection, &test_player(), Duration::from_millis(100))
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    assert_eq!(read_packet(&mut client).packet_id, 19);
    let disconnect = read_packet(&mut client);
    assert_eq!(disconnect.packet_id, 2);
    assert!(
        disconnect
            .payload
            .windows("コードを入力する時間".len())
            .any(|bytes| bytes == "コードを入力する時間".as_bytes())
    );
    assert!(server.join().unwrap().is_ok());
}

#[test]
fn session_hash_uses_signed_twos_complement() {
    assert_eq!(signed_sha1(b"abc"), "-5666c1c9b8f97e9545c1da8e87af3d93632f2763");
}

#[test]
fn authenticated_login_enters_configuration_without_play() {
    let player_id = Uuid::parse_str("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        serve_with_verifier(listener.accept().unwrap().0, |name, _, _| {
            assert_eq!(name, "TestPlayer");
            Ok(SessionProfile {
                id: player_id.simple().to_string(),
                name: "TestPlayer".into(),
            })
        })
    });
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
            name: "TestPlayer".into(),
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
    assert!(server.join().unwrap().is_ok());
}

fn read_packet(stream: &mut TcpStream) -> UncompressedPacket {
    RawPacket::read_sync(stream).unwrap().as_uncompressed().unwrap()
}

fn send_packet<W: Write, P: PacketId + Serialize>(stream: &mut W, packet: &P) {
    packet_from(packet).unwrap().write_sync(stream).unwrap();
}

fn send_click<W: Write>(stream: &mut W, id: &str, code: Option<&str>) {
    #[derive(serde::Serialize)]
    struct Code<'a> {
        code: &'a str,
    }
    let data = match code {
        Some(code) => {
            let named = na_nbt::to_vec_be(&Code { code }).unwrap();
            [&named[..1], &named[3..]].concat()
        }
        None => vec![0],
    };
    send_packet(
        stream,
        &ConfigurationDialogClick {
            action: id.into(),
            data,
        },
    );
}

fn test_player() -> VerifiedPlayer {
    VerifiedPlayer {
        uuid: Uuid::nil(),
        name: "TestPlayer".into(),
    }
}
