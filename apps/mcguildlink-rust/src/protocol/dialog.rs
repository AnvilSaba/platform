use std::io::{Error, ErrorKind};

use na_nbt::ValueRef;

pub const SUBMIT_ID: &str = "mcguildlink:submit_code";
pub const DONE_ID: &str = "mcguildlink:dialog_success";

pub enum Dialog {
    Code {
        initial: String,
        error: Option<&'static str>,
    },
    Success,
    AlreadyLinked,
    Blocked,
}

#[derive(serde::Serialize)]
struct DialogDocument<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'a str,
    #[serde(with = "na_nbt::list")]
    body: Vec<Message<'a>>,
    #[serde(with = "na_nbt::list")]
    inputs: Vec<TextInput<'a>>,
    action: Button<'a>,
    can_close_with_escape: i8,
    pause: i8,
    after_action: &'static str,
}

#[derive(serde::Serialize)]
struct Message<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    contents: &'a str,
    width: i32,
}

#[derive(serde::Serialize)]
struct TextInput<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    key: &'static str,
    width: i32,
    label: &'static str,
    initial: &'a str,
    max_length: i32,
}

#[derive(serde::Serialize)]
struct Button<'a> {
    label: &'a str,
    width: i32,
    action: CustomAction<'a>,
}

#[derive(serde::Serialize)]
struct CustomAction<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    id: &'a str,
}

impl Dialog {
    pub fn wire(&self) -> Result<Vec<u8>, Error> {
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
                kind: "minecraft:text",
                key: "code",
                width: 128,
                label: "コード",
                initial,
                max_length: 8,
            }],
            _ => Vec::new(),
        };
        let mut body = vec![Message {
            kind: "minecraft:plain_message",
            contents: message,
            width: 256,
        }];
        if let Self::Code { error: Some(error), .. } = self {
            body.push(Message {
                kind: "minecraft:plain_message",
                contents: error,
                width: 256,
            });
        }
        let document = DialogDocument {
            kind: "minecraft:notice",
            title,
            body,
            inputs,
            action: Button {
                label,
                width: 128,
                action: CustomAction {
                    kind: "minecraft:dynamic/custom",
                    id,
                },
            },
            can_close_with_escape: 0,
            pause: 1,
            after_action: "close",
        };
        let named = na_nbt::to_vec_be(&document).map_err(Error::other)?;
        if named.len() < 3 || named[0] != 10 {
            return Err(Error::new(ErrorKind::InvalidData, "dialog is not a compound tag"));
        }
        let mut wire = Vec::with_capacity(named.len() - 2);
        wire.push(named[0]);
        wire.extend_from_slice(&named[3..]);
        Ok(wire)
    }
}

#[derive(serde::Deserialize)]
struct Submission {
    code: String,
}

pub fn parse_submission(tag: &[u8]) -> Result<Option<String>, Error> {
    if tag.len() > 65_536 {
        return Err(Error::new(ErrorKind::InvalidData, "click payload too long"));
    }
    if tag.is_empty() || tag == [0] {
        return Ok(None);
    }
    if tag[0] != 10 {
        return Err(Error::new(ErrorKind::InvalidData, "click payload is not a compound"));
    }
    let mut named = Vec::with_capacity(tag.len() + 2);
    named.extend_from_slice(&[10, 0, 0]);
    named.extend_from_slice(&tag[1..]);
    let document = na_nbt::read_borrowed::<na_nbt::BE>(&named).map_err(Error::other)?;
    if document.root().get("code").is_none() {
        return Ok(None);
    }
    let submission: Submission = na_nbt::from_slice_be(&named).map_err(Error::other)?;
    Ok(Some(submission.code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use na_nbt::{BE, ValueRef, read_borrowed, tag};

    #[test]
    fn code_dialog_contains_input_and_custom_action() {
        let wire = Dialog::Code {
            initial: "ABCD1234".into(),
            error: None,
        }
        .wire()
        .unwrap();
        let mut named = vec![10, 0, 0];
        named.extend_from_slice(&wire[1..]);
        let document = read_borrowed::<BE>(&named).unwrap();
        let root = document.root();
        assert_eq!(
            root.get_::<tag::String>("type").unwrap().decode().unwrap(),
            "minecraft:notice"
        );
        assert_eq!(
            root.get("inputs")
                .unwrap()
                .get(0)
                .unwrap()
                .get_::<tag::String>("type")
                .unwrap()
                .decode()
                .unwrap(),
            "minecraft:text"
        );
        assert_eq!(
            root.get("body")
                .unwrap()
                .get(0)
                .unwrap()
                .get_::<tag::String>("type")
                .unwrap()
                .decode()
                .unwrap(),
            "minecraft:plain_message"
        );
        assert!(root.get("action").is_some());
    }

    #[test]
    fn custom_click_extracts_code_and_rejects_truncated_payload() {
        let mut tag = vec![10, 8, 0, 4];
        tag.extend_from_slice(b"code");
        tag.extend_from_slice(&8_u16.to_be_bytes());
        tag.extend_from_slice(b"SUCCESS1");
        tag.push(0);
        assert_eq!(parse_submission(&tag).unwrap(), Some("SUCCESS1".into()));
        tag.pop();
        assert!(parse_submission(&tag).is_err());
    }

    #[test]
    fn retry_keeps_guidance_and_adds_error() {
        let wire = Dialog::Code {
            initial: "BAD".into(),
            error: Some("無効なコードです。"),
        }
        .wire()
        .unwrap();
        let mut named = vec![10, 0, 0];
        named.extend_from_slice(&wire[1..]);
        let document = read_borrowed::<BE>(&named).unwrap();
        let body = document.root().get("body").unwrap();
        assert!(body.get(2).is_none());
        assert_eq!(
            body.get(0)
                .unwrap()
                .get_::<tag::String>("contents")
                .unwrap()
                .decode()
                .unwrap(),
            "発行されたコードを以下に入力してください。"
        );
        assert_eq!(
            body.get(1)
                .unwrap()
                .get_::<tag::String>("contents")
                .unwrap()
                .decode()
                .unwrap(),
            "無効なコードです。"
        );
    }
}
