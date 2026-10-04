use std::{collections::BTreeMap, sync::LazyLock};

use regex::Regex;

use super::{
    configuration::{PermissionVocabulary, PlanInput, RawResourceReference},
    domain::ManagementError,
    ids::GuildId,
};

pub(super) mod command;
mod delivery;

pub use delivery::{PreviewDeliveryStatus, PreviewResponder, deliver_preview};

pub const MAX_IMAGES: usize = 8;
// 25 MiB の request 上限から JSON と multipart ヘッダー用に 1 MiB を残す。
const MAX_MESSAGE_IMAGE_BYTES: usize = 24 * 1024 * 1024;
const MAX_TEXT_CHARACTERS: usize = 4000;
const MAX_COMPONENTS: usize = 40;
const MAX_GALLERY_ITEMS: usize = 10;

static IMAGE_REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^!\[(?<description>[^\]]*)\]\((?:attachment://)?(?<filename>[^\s()/:]+)\)$")
        .expect("固定の画像参照パターンは有効です")
});

#[derive(Debug, PartialEq, Eq)]
pub enum PreviewComponent {
    Text(String),
    Gallery(Vec<PreviewMedia>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct PreviewMedia {
    pub image_index: usize,
    pub description: String,
}

#[derive(Debug)]
pub struct PreviewImage {
    pub filename: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct PreviewMessage {
    pub logical_id: String,
    pub components: Vec<PreviewComponent>,
    pub image_indices: Vec<usize>,
}

pub fn preview_messages(
    vocabulary: &PermissionVocabulary,
    guild_id: GuildId,
    definition_toml: &str,
    state_json: &str,
    images: &[PreviewImage],
    attachment_size_limit: usize,
) -> Result<Vec<PreviewMessage>, ManagementError> {
    let input = PlanInput::parse(definition_toml, state_json, guild_id, vocabulary)?;
    if images.len() > MAX_IMAGES {
        return Err(ManagementError::InvalidInputFile(
            "画像は入力全体で最大8件です".to_owned(),
        ));
    }
    let mut image_names = BTreeMap::new();
    for (index, image) in images.iter().enumerate() {
        if image_names.insert(image.filename.as_str(), index).is_some() {
            return Err(ManagementError::InvalidInputFile(format!(
                "添付名 {} が重複しています",
                image.filename
            )));
        }
        if image.data.is_empty() || image.data.len() > attachment_size_limit {
            return Err(ManagementError::InvalidInputFile(format!(
                "画像 {} は空、または添付上限 {attachment_size_limit} bytes を超えています",
                image.filename
            )));
        }
        let data = &image.data;
        let supported = data.starts_with(b"\x89PNG\r\n\x1a\n")
            || data.starts_with(b"\xff\xd8\xff")
            || data.starts_with(b"GIF87a")
            || data.starts_with(b"GIF89a")
            || data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP");
        if !supported {
            return Err(ManagementError::InvalidInputFile(format!(
                "画像 {} の画像形式を確認できません。PNG・JPEG・GIF・WebP を使用してください",
                image.filename
            )));
        }
    }
    let mut result = Vec::new();
    for (set_id, set) in &input.definition.message_sets {
        if set.is_absent() {
            continue;
        }
        for message in set.body.as_deref().unwrap_or_default() {
            result.push(render_message(
                format!("message:{set_id}/{}", message.id),
                &message.body,
                &image_names,
                images,
            )?);
        }
    }
    for (thread_id, thread) in &input.definition.threads {
        if thread.is_absent() {
            continue;
        }
        for (index, message) in thread.body.as_deref().unwrap_or_default().iter().enumerate() {
            let body = if index == 0 {
                format!(
                    "補足：{}\n\n{}",
                    thread.name.as_deref().unwrap_or_default(),
                    message.body
                )
            } else {
                message.body.clone()
            };
            result.push(render_message(
                format!("thread-message:{thread_id}/{}", message.id),
                &body,
                &image_names,
                images,
            )?);
        }
    }
    Ok(result)
}

fn render_message(
    logical_id: String,
    body: &str,
    image_names: &BTreeMap<&str, usize>,
    images: &[PreviewImage],
) -> Result<PreviewMessage, ManagementError> {
    let mut message = PreviewMessage {
        logical_id,
        components: Vec::new(),
        image_indices: Vec::new(),
    };
    let mut text = String::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut inline_ticks = 0;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        offset += line.len();
        let trimmed = line.trim();
        let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
        let indented_code =
            indentation >= 4 || line[..indentation].len() < line.len() && line.as_bytes()[indentation] == b'\t';
        let marker = trimmed
            .as_bytes()
            .first()
            .copied()
            .filter(|byte| matches!(byte, b'`' | b'~'));
        let run = marker.map_or(0, |marker| trimmed.bytes().take_while(|byte| *byte == marker).count());
        if let Some((marker, length)) = fence {
            if indentation < 4
                && trimmed.as_bytes().first() == Some(&marker)
                && run >= length
                && trimmed[run..].trim().is_empty()
            {
                fence = None;
            }
            text.push_str(line);
            continue;
        }
        if !indented_code && inline_ticks == 0 && run >= 3 && (marker != Some(b'`') || !trimmed[run..].contains('`')) {
            fence = Some((marker.unwrap(), run));
            text.push_str(line);
            continue;
        }
        if !indented_code
            && inline_ticks == 0
            && let Some(capture) = IMAGE_REFERENCE.captures(trimmed)
        {
            let filename = &capture["filename"];
            let Some(&image_index) = image_names.get(filename) else {
                return Err(message_error(
                    &message.logical_id,
                    format!("画像 {filename} の添付がありません"),
                ));
            };
            let description = capture["description"].to_owned();
            if description.chars().count() > 1024 {
                return Err(message_error(
                    &message.logical_id,
                    "画像の代替テキストは1024文字以内です",
                ));
            }
            flush_text(&mut text, &mut message.components);
            let media = PreviewMedia {
                image_index,
                description,
            };
            match message.components.last_mut() {
                Some(PreviewComponent::Gallery(items)) if items.len() < MAX_GALLERY_ITEMS => items.push(media),
                _ => message.components.push(PreviewComponent::Gallery(vec![media])),
            }
            if !message.image_indices.contains(&image_index) {
                message.image_indices.push(image_index);
            }
        } else {
            if !indented_code && inline_ticks == 0 && trimmed.starts_with("![") && trimmed.ends_with(')') {
                return Err(message_error(
                    &message.logical_id,
                    "画像参照は ![代替テキスト](同名添付) の独立行で指定してください。外部 URL は取得しません",
                ));
            }
            text.push_str(line);
            if !indented_code {
                advance_inline_code(line, &body[offset..], &mut inline_ticks);
            }
        }
    }
    flush_text(&mut text, &mut message.components);
    let text_characters: usize = message
        .components
        .iter()
        .map(|component| match component {
            PreviewComponent::Text(text) => text.chars().count(),
            PreviewComponent::Gallery(_) => 0,
        })
        .sum();
    if text_characters > MAX_TEXT_CHARACTERS {
        return Err(message_error(
            &message.logical_id,
            "Text Display の合計が4000文字を超えています。本文配列を明示的に分けてください",
        ));
    }
    if message.components.len() > MAX_COMPONENTS {
        return Err(message_error(
            &message.logical_id,
            "コンポーネントが40件を超えています。本文配列を明示的に分けてください",
        ));
    }
    let image_bytes: usize = message
        .image_indices
        .iter()
        .map(|&index| images[index].data.len())
        .sum();
    if image_bytes > MAX_MESSAGE_IMAGE_BYTES {
        return Err(message_error(
            &message.logical_id,
            "投稿内の画像合計が24 MiBを超えています（request 上限のため1 MiBを予約）",
        ));
    }
    if message.components.is_empty() {
        return Err(message_error(&message.logical_id, "本文は空白だけにできません"));
    }
    Ok(message)
}

fn flush_text(text: &mut String, components: &mut Vec<PreviewComponent>) {
    if !text.trim().is_empty() {
        components.push(PreviewComponent::Text(std::mem::take(text)));
    } else {
        text.clear();
    }
}

fn advance_inline_code(line: &str, following: &str, inline_ticks: &mut usize) {
    let mut offset = 0;
    let bytes = line.as_bytes();
    while offset < bytes.len() {
        match bytes[offset] {
            b'\\' if *inline_ticks == 0 => offset += 2,
            b'`' => {
                let run = bytes[offset..].iter().take_while(|&&byte| byte == b'`').count();
                offset += run;
                if *inline_ticks == run {
                    *inline_ticks = 0;
                } else if *inline_ticks == 0 && has_closing_ticks(&line[offset..], following, run) {
                    *inline_ticks = run;
                }
            }
            _ => offset += 1,
        }
    }
}

fn has_closing_ticks(rest: &str, following: &str, length: usize) -> bool {
    for text in [rest, following] {
        let mut bytes = text.bytes().peekable();
        while let Some(byte) = bytes.next() {
            if byte != b'`' {
                continue;
            }
            let mut run = 1;
            while bytes.peek() == Some(&b'`') {
                bytes.next();
                run += 1;
            }
            if run == length {
                return true;
            }
        }
    }
    false
}

fn message_error(logical_id: &str, detail: impl std::fmt::Display) -> ManagementError {
    ManagementError::InvalidDefinition(format!("Message {logical_id}: {detail}"))
}

#[cfg(test)]
mod tests;
