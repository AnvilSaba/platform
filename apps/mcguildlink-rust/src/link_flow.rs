use std::{io::ErrorKind, time::Duration};
use tokio::time::Instant;

use crate::{
    AppResult, invalid,
    link_store::{CodeLinker, LinkResult},
    protocol::{
        Connection,
        connection::decode_exact,
        dialog::{self, Submission},
        packets::*,
    },
    session::SessionProfile,
};

const INPUT_TIMEOUT: Duration = Duration::from_secs(300);

pub(crate) async fn configuration<L>(connection: &mut Connection, player: &SessionProfile, linker: &L) -> AppResult<()>
where
    L: CodeLinker,
{
    configuration_with_timeout(connection, player, INPUT_TIMEOUT, linker).await
}

pub(crate) async fn configuration_with_timeout<L>(
    connection: &mut Connection,
    player: &SessionProfile,
    timeout: Duration,
    linker: &L,
) -> AppResult<()>
where
    L: CodeLinker,
{
    connection
        .send(&ConfigurationDialog {
            document: dialog::Dialog::Code {
                initial: "",
                error: None,
            }
            .wire(),
        })
        .await?;
    let deadline = Instant::now() + timeout;
    let mut completed = false;
    loop {
        let packet = match connection.read_until(deadline).await {
            Ok(packet) => packet,
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                connection.send(&ConfigurationDisconnect { reason: NbtTextComponent::new("コードを入力する時間が長過ぎたため、切断されました。もう一度接続してコードを入力してください。すでにコードを発行している場合、コードの再発行は不要です。") }).await?;
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
            ConfigurationKeepAliveResponse::PACKET_ID
                if decode_exact::<ConfigurationKeepAliveResponse>(&packet.payload)?.id == 0 => {}
            ConfigurationDialogClick::<Submission>::PACKET_ID => {
                let click: ConfigurationDialogClick<Submission> = decode_exact(&packet.payload)?;
                let action = click.action;
                let code = click.data.code.unwrap_or_default();
                if action == dialog::DONE_ID && completed {
                    connection
                        .send(&ConfigurationDisconnect {
                            reason: NbtTextComponent::new("正常に切断されました。"),
                        })
                        .await?;
                    return Ok(());
                }
                if action != dialog::SUBMIT_ID || completed {
                    return Err(invalid("unexpected dialog action"));
                }
                let code = code.trim();
                eprintln!(
                    "Code submitted by authenticated player {} ({})",
                    player.name.as_ref(),
                    player.id
                );
                if code.len() > 8 {
                    connection
                        .send(&ConfigurationDialog {
                            document: dialog::Dialog::Code {
                                initial: "",
                                error: Some("無効なコードです。もう一度入力してください。"),
                            }
                            .wire(),
                        })
                        .await?;
                    continue;
                }
                let next = if code.is_empty() {
                    dialog::Dialog::Code {
                        initial: "",
                        error: Some("コードが空です。もう一度入力してください。"),
                    }
                } else {
                    match linker.consume(code, player).await? {
                        LinkResult::Success(username) => {
                            completed = true;
                            dialog::Dialog::Success {
                                message: format!("{username} との紐付けが完了しました。"),
                            }
                        }
                        LinkResult::AlreadyLinked => {
                            completed = true;
                            dialog::Dialog::AlreadyLinked
                        }
                        LinkResult::Blocked => {
                            completed = true;
                            dialog::Dialog::Blocked
                        }
                        LinkResult::InvalidCode => dialog::Dialog::Code {
                            initial: code,
                            error: Some("無効なコードです。もう一度入力してください。"),
                        },
                    }
                };
                connection.send(&ConfigurationDialog { document: next.wire() }).await?;
            }
            ConfigurationFinishAcknowledged::PACKET_ID => return Err(invalid("Play transition is forbidden")),
            _ => return Err(invalid("unexpected Configuration packet")),
        }
    }
}
