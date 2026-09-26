use std::{
    io::ErrorKind,
    time::{Duration, Instant},
};

use crate::{
    AppResult, invalid,
    protocol::{Connection, connection::decode_exact, dialog, packets::*},
    server::VerifiedPlayer,
};

const INPUT_TIMEOUT: Duration = Duration::from_secs(300);

pub(crate) fn configuration(connection: &mut Connection, player: &VerifiedPlayer) -> AppResult<()> {
    configuration_with_timeout(connection, player, INPUT_TIMEOUT)
}

pub(crate) fn configuration_with_timeout(
    connection: &mut Connection,
    player: &VerifiedPlayer,
    timeout: Duration,
) -> AppResult<()> {
    connection.send(&ConfigurationDialog {
        document: RemainingBytes(
            dialog::Dialog::Code {
                initial: String::new(),
                error: None,
            }
            .wire()?,
        ),
    })?;
    let deadline = Instant::now() + timeout;
    let mut completed = false;
    loop {
        let packet = match connection.read_until(deadline) {
            Ok(packet) => packet,
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                connection.send(&ConfigurationDisconnect { reason: RemainingBytes(text_component("コードを入力する時間が長過ぎたため、切断されました。もう一度接続してコードを入力してください。すでにコードを発行している場合、コードの再発行は不要です。")?) })?;
                return Ok(());
            }
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        match packet.packet_id {
            ConfigurationClientInformation::PACKET_ID
            | ConfigurationCustomPayload::PACKET_ID
            | ConfigurationPong::PACKET_ID
            | ConfigurationResourcePackResponse::PACKET_ID
            | ConfigurationKnownPacks::PACKET_ID => {} // optional Configuration messages
            ConfigurationKeepaliveResponse::PACKET_ID
                if decode_exact::<ConfigurationKeepaliveResponse>(&packet.payload)?.id == 0 => {}
            ConfigurationDialogClick::PACKET_ID => {
                let click: ConfigurationDialogClick = decode_exact(&packet.payload)?;
                let action = click.action;
                let code = dialog::parse_submission(&click.data)?;
                if action == dialog::DONE_ID && completed {
                    connection.send(&ConfigurationDisconnect {
                        reason: RemainingBytes(text_component("正常に切断されました。")?),
                    })?;
                    return Ok(());
                }
                if action != dialog::SUBMIT_ID || completed {
                    return Err(invalid("unexpected dialog action"));
                }
                let Some(code) = code else {
                    connection.send(&ConfigurationDialog {
                        document: RemainingBytes(
                            dialog::Dialog::Code {
                                initial: String::new(),
                                error: Some("コードを受け取れませんでした。もう一度入力してください。"),
                            }
                            .wire()?,
                        ),
                    })?;
                    continue;
                };
                let code = code.trim();
                eprintln!(
                    "Code submitted by authenticated player {} ({})",
                    player.name, player.uuid
                );
                if code.len() > 8 {
                    connection.send(&ConfigurationDialog {
                        document: RemainingBytes(
                            dialog::Dialog::Code {
                                initial: String::new(),
                                error: Some("無効なコードです。もう一度入力してください。"),
                            }
                            .wire()?,
                        ),
                    })?;
                    continue;
                }
                let next = match code {
                    "SUCCESS1" => {
                        completed = true;
                        dialog::Dialog::Success
                    }
                    "ALREADY1" => {
                        completed = true;
                        dialog::Dialog::AlreadyLinked
                    }
                    "BLOCKED1" => {
                        completed = true;
                        dialog::Dialog::Blocked
                    }
                    "" => dialog::Dialog::Code {
                        initial: String::new(),
                        error: Some("コードが空です。もう一度入力してください。"),
                    },
                    _ => dialog::Dialog::Code {
                        initial: code.to_owned(),
                        error: Some("無効なコードです。もう一度入力してください。"),
                    },
                };
                connection.send(&ConfigurationDialog {
                    document: RemainingBytes(next.wire()?),
                })?;
            }
            ConfigurationFinishAcknowledged::PACKET_ID => return Err(invalid("Play transition is forbidden")),
            _ => return Err(invalid("unexpected Configuration packet")),
        }
    }
}

fn text_component(text: &str) -> AppResult<Vec<u8>> {
    let named = na_nbt::to_vec_be(&text.to_owned())?;
    if named.len() < 3 || named[0] != 8 {
        return Err(invalid("text component is not a string tag"));
    }
    Ok([&named[..1], &named[3..]].concat())
}
