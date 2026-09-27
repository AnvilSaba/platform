use std::io::{Error, Read, Write};

use fastnbt::{DeOpts, SerOpts};
use mc_protocol::{ser::SerializationError, varint::VarInt};

pub(crate) const SUBMIT_ID: &str = "mcguildlink:submit_code";
pub(crate) const DONE_ID: &str = "mcguildlink:dialog_success";

const NOTICE: &str = "minecraft:notice";
const PLAIN_MESSAGE: &str = "minecraft:plain_message";
const TEXT_INPUT: &str = "minecraft:text";
const DYNAMIC_CUSTOM: &str = "minecraft:dynamic/custom";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct DialogDocument<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    title: &'a str,
    body: Vec<Message<'a>>,
    inputs: Vec<TextInput<'a>>,
    action: Button<'a>,
    can_close_with_escape: i8,
    pause: i8,
    after_action: &'a str,
}

impl mc_protocol::ser::Serialize for DialogDocument<'_> {
    fn serialize<W: Write + Unpin>(&self, writer: &mut W) -> Result<(), SerializationError> {
        fastnbt::to_writer_with_opts(writer, self, SerOpts::network_nbt()).map_err(Error::other)?;
        Ok(())
    }
}

impl mc_protocol::ser::Deserialize for DialogDocument<'_> {
    fn deserialize<R: Read + Unpin>(reader: &mut R) -> Result<Self, SerializationError> {
        Ok(fastnbt::from_reader_with_opts(reader, DeOpts::network_nbt()).map_err(Error::other)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Message<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    contents: &'a str,
    width: i32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct TextInput<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    key: &'a str,
    width: i32,
    label: &'a str,
    initial: &'a str,
    max_length: i32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Button<'a> {
    label: &'a str,
    width: i32,
    action: CustomAction<'a>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CustomAction<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    id: &'a str,
}

pub(crate) enum Dialog<'a> {
    Code { initial: &'a str, error: Option<&'a str> },
    Success,
    AlreadyLinked,
    Blocked,
}

impl Dialog<'_> {
    pub(crate) fn wire(&self) -> DialogDocument<'_> {
        let (title, message, label, id) = match self {
            Self::Code { .. } => (
                "アカウント認証",
                "発行されたコードを以下に入力してください。",
                "送信",
                SUBMIT_ID,
            ),
            Self::Success => (
                "成功しました。",
                "検証用ユーザー との紐付けが完了しました。",
                "切断",
                DONE_ID,
            ),
            Self::AlreadyLinked => (
                "既に紐付け済みです。",
                "このMinecraftアカウントとこのDiscordアカウントは既に紐付けられています。",
                "切断",
                DONE_ID,
            ),
            Self::Blocked => (
                "紐付けできません。",
                "このアカウントはブロックされているため、紐付けを続行できません。",
                "切断",
                DONE_ID,
            ),
        };
        let inputs = match self {
            Self::Code { initial, .. } => vec![TextInput {
                kind: TEXT_INPUT,
                key: "code",
                width: 128,
                label: "コード",
                initial,
                max_length: 8,
            }],
            _ => Vec::new(),
        };
        let mut body = vec![Message {
            kind: PLAIN_MESSAGE,
            contents: message,
            width: 256,
        }];
        if let Self::Code { error: Some(error), .. } = self {
            body.push(Message {
                kind: PLAIN_MESSAGE,
                contents: error,
                width: 256,
            });
        }

        DialogDocument {
            kind: NOTICE,
            title,
            body,
            inputs,
            action: Button {
                label,
                width: 128,
                action: CustomAction {
                    kind: DYNAMIC_CUSTOM,
                    id,
                },
            },
            can_close_with_escape: 0,
            pause: 1,
            after_action: "close",
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct Submission {
    pub(crate) code: Option<String>,
}

impl mc_protocol::ser::Deserialize for Submission {
    fn deserialize<R: Read + Unpin>(reader: &mut R) -> Result<Self, SerializationError> {
        let length = VarInt::read_sync(reader)?.0;
        if !(0..=65_536).contains(&length) {
            return Err(Error::new(std::io::ErrorKind::InvalidData, "click payload too long").into());
        }
        let mut tag = vec![0; length as usize];
        reader.read_exact(&mut tag)?;
        if tag == [0] {
            return Ok(Self { code: None });
        }
        Ok(fastnbt::from_bytes_with_opts(&tag, DeOpts::network_nbt()).map_err(Error::other)?)
    }
}

impl mc_protocol::ser::Serialize for Submission {
    fn serialize<W: Write + Unpin>(&self, writer: &mut W) -> Result<(), SerializationError> {
        let tag = if self.code.is_none() {
            vec![0]
        } else {
            fastnbt::to_bytes_with_opts(&self, SerOpts::network_nbt()).map_err(Error::other)?
        };
        mc_protocol::ser::Serialize::serialize(&tag, writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::connection::{decode_exact, packet_from};
    use crate::protocol::packets::ConfigurationDialogClick;

    #[test]
    fn code_dialog_contains_input_and_custom_action() {
        let doc = Dialog::Code {
            initial: "ABCD1234",
            error: None,
        };
        let wire = doc.wire();

        assert_eq!(wire.kind, NOTICE);
        assert_eq!(wire.inputs.first().unwrap().kind, TEXT_INPUT);
        assert_eq!(wire.body.first().unwrap().kind, PLAIN_MESSAGE);

        let bytes = fastnbt::to_bytes_with_opts(&wire, SerOpts::network_nbt()).unwrap();
        let fastnbt::Value::Compound(root) = fastnbt::from_bytes_with_opts(&bytes, DeOpts::network_nbt()).unwrap()
        else {
            panic!("dialog root must be a compound");
        };
        assert_eq!(root["type"].as_str(), Some("minecraft:notice"));
        let fastnbt::Value::List(inputs) = &root["inputs"] else {
            panic!("dialog inputs must be a list");
        };
        let fastnbt::Value::Compound(input) = &inputs[0] else {
            panic!("dialog input must be a compound");
        };
        assert_eq!(input["type"].as_str(), Some("minecraft:text"));
    }

    #[test]
    fn retry_keeps_guidance_and_adds_error() {
        let doc = Dialog::Code {
            initial: "BAD",
            error: Some("無効なコードです。"),
        };
        let wire = doc.wire();

        assert!(wire.body.get(2).is_none());
        assert_eq!(
            wire.body.first().unwrap().contents,
            "発行されたコードを以下に入力してください。"
        );
        assert_eq!(wire.body.get(1).unwrap().contents, "無効なコードです。");
    }

    #[test]
    fn submission_decodes_length_prefixed_nbt() {
        let nbt = fastnbt::to_bytes_with_opts(
            &Submission {
                code: Some("SUCCESS1".into()),
            },
            SerOpts::network_nbt(),
        )
        .unwrap();
        let packet = packet_from(&ConfigurationDialogClick {
            action: SUBMIT_ID.into(),
            data: nbt,
        })
        .unwrap();
        let click: ConfigurationDialogClick<Submission> = decode_exact(&packet.payload).unwrap();
        assert_eq!(click.action, SUBMIT_ID);
        assert_eq!(click.data.code.as_deref(), Some("SUCCESS1"));

        let packet = packet_from(&ConfigurationDialogClick {
            action: DONE_ID.into(),
            data: vec![0_u8],
        })
        .unwrap();
        let click: ConfigurationDialogClick<Submission> = decode_exact(&packet.payload).unwrap();
        assert_eq!(click.data.code, None);
    }
}
