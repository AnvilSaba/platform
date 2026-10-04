use super::*;

const STATE: &str = r#"{"schema_version":1,"guild_id":"100","roles":{},"channels":{"rules":"200"}}"#;
const DEFINITION: &str = r##"
schema_version = 1
[channels.rules]
mode = "reference"
[message_sets.guidelines]
channel = "rules"
[[message_sets.guidelines.body]]
id = "basic"
body = "# 基本ルール\n<@123> <@&456> @everyone\n![案内](guide.png)\n続きを読んでください。"
[[message_sets.guidelines.body]]
id = "questions"
body = "![再利用](guide.png)"
"##;

fn preview(definition: &str, images: &[PreviewImage]) -> Result<Vec<PreviewMessage>, ManagementError> {
    preview_messages(
        &PermissionVocabulary::from_names(["VIEW_CHANNEL"]).unwrap(),
        GuildId::new(100),
        definition,
        STATE,
        images,
        10 * 1024 * 1024,
    )
}

fn image(filename: &str) -> PreviewImage {
    PreviewImage {
        filename: filename.to_owned(),
        data: b"\x89PNG\r\n\x1a\n".to_vec(),
    }
}

/// 配列順に一要素一投稿を描画し、メンション表記と画像の位置・共有を保持する。
#[test]
fn preview_preserves_post_order_mentions_and_reuses_images_in_body_order() {
    let images = [image("guide.png")];
    let messages = preview(DEFINITION, &images).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].logical_id, "message:guidelines/basic");
    assert_eq!(messages[1].logical_id, "message:guidelines/questions");
    assert_eq!(
        messages[0].components,
        vec![
            PreviewComponent::Text("# 基本ルール\n<@123> <@&456> @everyone\n".to_owned()),
            PreviewComponent::Gallery(vec![PreviewMedia {
                image_index: 0,
                description: "案内".to_owned()
            }]),
            PreviewComponent::Text("続きを読んでください。".to_owned()),
        ]
    );
    assert_eq!(messages[0].image_indices, vec![0]);
    assert_eq!(messages[1].image_indices, vec![0]);
    assert_eq!(images[0].data, b"\x89PNG\r\n\x1a\n");
}

fn single_message(body: &str) -> String {
    format!(
        "schema_version = 1\n[channels.rules]\nmode = \"reference\"\n[message_sets.guidelines]\nchannel = \"rules\"\n[[message_sets.guidelines.body]]\nid = \"basic\"\nbody = {}\n",
        toml::Value::String(body.to_owned())
    )
}

/// コード中の例示を画像化せず、通常の画像参照だけを描画する。
#[test]
fn preview_keeps_fenced_indented_inline_code_and_unmatched_backticks_literal() {
    let body = "````markdown\n![例](missing.png)\n```\n````\n~~~\n![例](missing.png)\n~~~\n    ![例](missing.png)\n\t![例](missing.png)\n`![例](missing.png)`\n``複数行\n![例](missing.png)\n``\n閉じていない ` は本文\n![表示](guide.png)\n";
    let messages = preview(&single_message(body), &[image("guide.png")]).unwrap();
    assert_eq!(messages[0].image_indices, vec![0]);
    assert!(
        matches!(&messages[0].components[0], PreviewComponent::Text(text) if text.contains("\t![例](missing.png)") && text.contains("閉じていない ` は本文"))
    );
    assert!(matches!(&messages[0].components[1], PreviewComponent::Gallery(items) if items[0].description == "表示"));
}

/// 入力全体で画像上限・重複名・ファイル種別を検証し、存在しない画像も対象 Message とともに診断する。
#[test]
fn preview_diagnoses_invalid_images_and_missing_references_before_any_output() {
    let error = preview(DEFINITION, &[]).unwrap_err().to_string();
    assert!(error.contains("message:guidelines/basic") && error.contains("guide.png"));
    let error = preview(DEFINITION, &[image("guide.png"), image("guide.png")])
        .unwrap_err()
        .to_string();
    assert!(error.contains("guide.png") && error.contains("重複"));
    let images = (0..9).map(|index| image(&format!("{index}.png"))).collect::<Vec<_>>();
    assert!(
        preview(DEFINITION, &images)
            .unwrap_err()
            .to_string()
            .contains("最大8件")
    );
    let invalid = PreviewImage {
        filename: "guide.png".to_owned(),
        data: b"text pretending to be an image".to_vec(),
    };
    assert!(
        preview(DEFINITION, &[invalid])
            .unwrap_err()
            .to_string()
            .contains("画像形式")
    );
    assert!(
        preview(&single_message("![外部](https://example.com/photo.png)"), &[])
            .unwrap_err()
            .to_string()
            .contains("同名添付")
    );
}

/// Unicode 本文の合計と画像・コンポーネント・添付上限を Message ID 付きで診断し、暗黙分割しない。
#[test]
fn preview_checks_rendered_text_components_galleries_and_upload_limits() {
    let images = [image("guide.png")];
    assert!(preview(&single_message(&"あ".repeat(4000)), &[]).is_ok());
    let error = preview(&single_message(&"あ".repeat(4001)), &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("message:guidelines/basic") && error.contains("4000"));
    let error = preview(&single_message(&"本文\n![再利用](guide.png)\n".repeat(21)), &images)
        .unwrap_err()
        .to_string();
    assert!(error.contains("message:guidelines/basic") && error.contains("40件"));
    let messages = preview(&single_message(&"![再利用](guide.png)\n".repeat(11)), &images).unwrap();
    assert_eq!(messages[0].image_indices, vec![0]);
    assert!(matches!(&messages[0].components[0], PreviewComponent::Gallery(items) if items.len() == 10));
    assert!(matches!(&messages[0].components[1], PreviewComponent::Gallery(items) if items.len() == 1));
    let error = preview(
        &single_message(&format!("![{}](guide.png)", "あ".repeat(1025))),
        &images,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("1024"));
    let error = preview_messages(
        &PermissionVocabulary::from_names(["VIEW_CHANNEL"]).unwrap(),
        GuildId::new(100),
        DEFINITION,
        STATE,
        &images,
        7,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("guide.png") && error.contains("7 bytes"));
    let mut large = image("large.png");
    large.data.resize(9 * 1024 * 1024, 0);
    let mut other = image("other.png");
    other.data.resize(9 * 1024 * 1024, 0);
    let mut third = image("third.png");
    third.data.resize(9 * 1024 * 1024, 0);
    let error = preview(
        &single_message("![a](large.png)\n![b](other.png)\n![c](third.png)"),
        &[large, other, third],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("message:guidelines/basic") && error.contains("24 MiB"));
}

/// 本文必須・重複 ID・Guild 不一致を拒否し、プレビューでは入力 state を変更しない。
#[test]
fn preview_validates_definitions_and_guild_without_changing_state() {
    let images = [image("guide.png")];
    assert!(
        preview(&DEFINITION.replace("id = \"questions\"", "id = \"basic\""), &images)
            .unwrap_err()
            .to_string()
            .contains("重複")
    );
    assert!(preview(&DEFINITION.replace("id = \"questions\"", ""), &images).is_err());
    assert!(preview(&single_message(" \n\t"), &[]).is_err());
    let error = preview_messages(
        &PermissionVocabulary::from_names(["VIEW_CHANNEL"]).unwrap(),
        GuildId::new(999),
        DEFINITION,
        STATE,
        &images,
        1024,
    )
    .unwrap_err();
    assert!(matches!(error, ManagementError::GuildMismatch { .. }));
    let before = STATE.to_owned();
    preview_messages(
        &PermissionVocabulary::from_names(["VIEW_CHANNEL"]).unwrap(),
        GuildId::new(100),
        DEFINITION,
        &before,
        &images,
        1024,
    )
    .unwrap();
    assert_eq!(before, STATE);
}

/// 補足本文は参照元の数によらず各 Thread につき一度、配列順に表示する。
#[test]
fn preview_shows_each_thread_once_and_skips_absent_content() {
    let definition = format!(
        "{DEFINITION}\n[threads.details]\nchannel = \"rules\"\nname = \"基本ルールの補足\"\n[[threads.details.body]]\nid = \"example\"\nbody = \"![説明](guide.png)\"\n[[threads.details.body]]\nid = \"second\"\nbody = \"続き\"\n[message_sets.deleted]\nensure = \"absent\"\n[threads.deleted]\nensure = \"absent\"\n"
    );
    let messages = preview(&definition, &[image("guide.png")]).unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[2].logical_id, "thread-message:details/example");
    assert_eq!(
        messages[2].components[0],
        PreviewComponent::Text("補足：基本ルールの補足\n\n".to_owned())
    );
    assert_eq!(messages[3].components, vec![PreviewComponent::Text("続き".to_owned())]);
}

/// 配布するサンプルを空の対応表で投入でき、画像を3投稿に共有する。
#[test]
fn preview_sample_renders_with_an_explicit_empty_mapping() {
    let definition = include_str!("../../../../../../docs/examples/discord-message-preview.toml");
    let messages = preview_messages(
        &PermissionVocabulary::from_names(["VIEW_CHANNEL"]).unwrap(),
        GuildId::new(100),
        definition,
        r#"{"schema_version":1,"guild_id":"100","roles":{},"channels":{}}"#,
        &[image("guide.png")],
        1024,
    )
    .unwrap();
    assert_eq!(messages.len(), 3);
    assert!(messages.iter().all(|message| message.image_indices == vec![0]));
}

/// original response と followup の V2 payload に通知抑制・個別 ephemeral・添付の重複排除を明示する。
#[test]
fn preview_discord_payloads_preserve_mentions_and_disable_notifications() {
    let images = vec![image("guide.png")];
    let messages = preview(
        &single_message("<@123> <@&456> @everyone\n![a](guide.png)\n![b](guide.png)"),
        &images,
    )
    .unwrap();
    let images = command::prepare_uploads(images);
    let original = serde_json::to_value(command::original_response(&messages[0], &images)).unwrap();
    let followup = serde_json::to_value(command::followup_response(&messages[0], &images)).unwrap();
    for payload in [&original, &followup] {
        assert_eq!(payload["allowed_mentions"]["parse"], serde_json::json!([]));
        assert_eq!(payload["components"][0]["content"], "<@123> <@&456> @everyone\n");
        assert_eq!(
            payload["components"][1]["items"][0]["media"]["url"],
            "attachment://guide.png"
        );
        assert_eq!(payload["attachments"].as_array().unwrap().len(), 1);
        assert!(payload.get("content").is_none());
        assert!(
            payload
                .get("embeds")
                .is_none_or(|embeds| embeds.is_null() || embeds.as_array().is_some_and(Vec::is_empty))
        );
    }
    assert_eq!(original["flags"], 32768);
    assert_eq!(followup["flags"], 32832);
    let command = crate::features::commands()
        .into_iter()
        .find(|command| command.name == "preview")
        .unwrap();
    assert!(command.owners_only && command.guild_only && command.ephemeral);
    assert_eq!(command.parameters.len(), 10);
}

/// original を完成させてから追加表示し、返却失敗や期限切れでは残りを送らない。
#[tokio::test]
async fn preview_delivery_edits_original_before_followups_and_stops_at_failure_or_deadline() {
    use std::sync::Mutex;
    use tokio::time::{Duration, Instant};

    struct FakeResponder {
        outputs: Mutex<Vec<String>>,
        fail_followup: bool,
        wait_forever: bool,
    }
    impl PreviewResponder for FakeResponder {
        async fn edit_original(&self, message: &PreviewMessage) -> Result<(), String> {
            if self.wait_forever {
                std::future::pending::<()>().await;
            }
            self.outputs
                .lock()
                .unwrap()
                .push(format!("original:{}", message.logical_id));
            Ok(())
        }
        async fn create_followup(&self, message: &PreviewMessage) -> Result<(), String> {
            if self.fail_followup {
                return Err("Discord failure".to_owned());
            }
            self.outputs
                .lock()
                .unwrap()
                .push(format!("followup:{}", message.logical_id));
            Ok(())
        }
    }
    let messages = preview(DEFINITION, &[image("guide.png")]).unwrap();
    let responder = FakeResponder {
        outputs: Mutex::new(Vec::new()),
        fail_followup: false,
        wait_forever: false,
    };
    let result = deliver_preview(&responder, &messages, Instant::now() + Duration::from_secs(1)).await;
    assert_eq!(result.sent, 2);
    assert_eq!(result.status, PreviewDeliveryStatus::Complete);
    assert_eq!(
        *responder.outputs.lock().unwrap(),
        vec![
            "original:message:guidelines/basic",
            "followup:message:guidelines/questions"
        ]
    );
    let responder = FakeResponder {
        outputs: Mutex::new(Vec::new()),
        fail_followup: true,
        wait_forever: false,
    };
    let result = deliver_preview(&responder, &messages, Instant::now() + Duration::from_secs(1)).await;
    assert_eq!(result.sent, 1);
    assert_eq!(
        result.status,
        PreviewDeliveryStatus::Failed("Discord failure".to_owned())
    );
    let responder = FakeResponder {
        outputs: Mutex::new(Vec::new()),
        fail_followup: false,
        wait_forever: true,
    };
    let result = deliver_preview(&responder, &messages, Instant::now() + Duration::from_millis(20)).await;
    assert_eq!(result.status, PreviewDeliveryStatus::DeadlineExceeded);
    assert_eq!(result.sent, 0);
    assert!(responder.outputs.lock().unwrap().is_empty());
    let result = deliver_preview(&responder, &messages, Instant::now()).await;
    assert_eq!(result.status, PreviewDeliveryStatus::DeadlineExceeded);
    assert!(responder.outputs.lock().unwrap().is_empty());
}
