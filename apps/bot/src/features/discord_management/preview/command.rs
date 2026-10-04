use std::{collections::BTreeSet, time::Duration};

use serenity::all::{
    Attachment, CommandInteraction, CreateAttachment, CreateComponent, CreateInteractionResponseFollowup,
    CreateMediaGallery, CreateMediaGalleryItem, CreateTextDisplay, CreateUnfurledMediaItem, EditInteractionResponse,
    MessageFlags, Timestamp,
};
use serenity::http::Http;
use tokio::time::{Instant, timeout_at};

use crate::{
    app::{AppContext, AppError},
    utils::create_safe_allowed_mentions,
};

use super::{
    MAX_IMAGES, PreviewComponent, PreviewDeliveryStatus, PreviewImage, PreviewMessage, PreviewResponder,
    deliver_preview, preview_messages,
};
use crate::features::discord_management::{adapter::SerenityManagementAdapter, domain::ManagementError, ids::GuildId};

const MAX_TEXT_FILE_BYTES: usize = 1024 * 1024;
const PROCESSING_BUDGET: Duration = Duration::from_secs(10 * 60);
const RESULT_BUDGET: Duration = Duration::from_secs(2 * 60);

/// TOML の管理本文と最大8件の画像を、実行者だけに表示します。
#[poise::command(slash_command, ephemeral, guild_only, owners_only)]
pub async fn preview(
    ctx: AppContext<'_>,
    #[description = "管理本文の TOML"] definition: Attachment,
    #[description = "実行 Guild の state JSON"] state: Attachment,
    #[description = "本文から参照する画像1"] image1: Option<Attachment>,
    #[description = "本文から参照する画像2"] image2: Option<Attachment>,
    #[description = "本文から参照する画像3"] image3: Option<Attachment>,
    #[description = "本文から参照する画像4"] image4: Option<Attachment>,
    #[description = "本文から参照する画像5"] image5: Option<Attachment>,
    #[description = "本文から参照する画像6"] image6: Option<Attachment>,
    #[description = "本文から参照する画像7"] image7: Option<Attachment>,
    #[description = "本文から参照する画像8"] image8: Option<Attachment>,
) -> Result<(), AppError> {
    let poise::Context::Application(app) = ctx else {
        unreachable!("slash_command only")
    };
    let received_age = Duration::from_millis(
        Timestamp::now()
            .unix_timestamp_millis()
            .saturating_sub(app.interaction.id.created_at().unix_timestamp_millis())
            .max(0) as u64,
    );
    let now = Instant::now();
    let acknowledgement = now + Duration::from_secs(3).saturating_sub(received_age);
    timeout_at(acknowledgement, ctx.defer_ephemeral()).await??;
    let processing = now + PROCESSING_BUDGET.saturating_sub(received_age);
    let response = now + (PROCESSING_BUDGET + RESULT_BUDGET).saturating_sub(received_age);
    let attachments = [image1, image2, image3, image4, image5, image6, image7, image8]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let attachment_size_limit = app.interaction.attachment_size_limit as usize;
    let preparation = async {
        validate_attachments(&definition, &state, &attachments, attachment_size_limit)?;
        let definition = download_text(&definition).await?;
        let state = download_text(&state).await?;
        let mut images = Vec::new();
        for attachment in &attachments {
            let data = attachment.download().await.map_err(|error| {
                ManagementError::InvalidInputFile(format!("画像 {} の取得に失敗しました: {error}", attachment.filename))
            })?;
            images.push(PreviewImage {
                filename: attachment.filename.to_string(),
                data,
            });
        }
        let source = SerenityManagementAdapter::new(ctx.http(), ctx.cache().current_user().id);
        let messages = preview_messages(
            &source.permission_vocabulary(),
            GuildId::from(ctx.guild_id().expect("guild_only command")),
            &definition,
            &state,
            &images,
            attachment_size_limit,
        )?;
        Ok::<_, ManagementError>((messages, prepare_uploads(images)))
    };
    let (messages, uploads) = match timeout_at(processing, preparation).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            timeout_at(
                response,
                app.interaction.edit_response(
                    ctx.http(),
                    EditInteractionResponse::new()
                        .content(
                            format!("入力を確認してください。\n{error}")
                                .chars()
                                .take(2000)
                                .collect::<String>(),
                        )
                        .allowed_mentions(create_safe_allowed_mentions()),
                ),
            )
            .await??;
            return Ok(());
        }
        Err(_) => {
            timeout_at(
                response,
                app.interaction.edit_response(
                    ctx.http(),
                    EditInteractionResponse::new()
                        .content("入力の受領・描画が処理期限に達しました。画像と本文を確認して再実行してください。")
                        .allowed_mentions(create_safe_allowed_mentions()),
                ),
            )
            .await??;
            return Ok(());
        }
    };
    if messages.is_empty() {
        timeout_at(
            response,
            app.interaction.edit_response(
                ctx.http(),
                EditInteractionResponse::new()
                    .content("表示する管理本文がありません。")
                    .allowed_mentions(create_safe_allowed_mentions()),
            ),
        )
        .await??;
        return Ok(());
    }
    let responder = DiscordPreviewResponder {
        interaction: app.interaction,
        http: ctx.http(),
        uploads: &uploads,
    };
    let delivery = deliver_preview(&responder, &messages, processing).await;
    let reason = match delivery.status {
        PreviewDeliveryStatus::Complete => return Ok(()),
        PreviewDeliveryStatus::DeadlineExceeded => "処理期限に達しました。".to_owned(),
        PreviewDeliveryStatus::Failed(error) => format!("Discord の返却に失敗しました: {error}"),
    };
    let content = format!(
        "{reason}\n{} 投稿を返却済みです。残りは {} からです。返却中の投稿は応答不明の可能性があります。",
        delivery.sent, messages[delivery.sent].logical_id
    )
    .chars()
    .take(2000)
    .collect::<String>();
    // HTTP 応答消失では V2 へ切替済みの可能性があるので、診断も Text Display にする。
    let diagnostic = PreviewMessage {
        logical_id: String::new(),
        components: vec![PreviewComponent::Text(content)],
        image_indices: Vec::new(),
    };
    if delivery.sent == 0 {
        timeout_at(response, responder.edit_original(&diagnostic))
            .await?
            .map_err(anyhow::Error::msg)?;
    } else {
        timeout_at(response, responder.create_followup(&diagnostic))
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

fn validate_attachments(
    definition: &Attachment,
    state: &Attachment,
    images: &[Attachment],
    limit: usize,
) -> Result<(), ManagementError> {
    if images.len() > MAX_IMAGES {
        return Err(ManagementError::InvalidInputFile("画像は最大8件です".to_owned()));
    }
    let mut names = BTreeSet::new();
    for attachment in std::iter::once(definition).chain(std::iter::once(state)).chain(images) {
        if !names.insert(attachment.filename.as_str()) {
            return Err(ManagementError::InvalidInputFile(format!(
                "添付名 {} が重複しています",
                attachment.filename
            )));
        }
        if attachment.size as usize > limit {
            return Err(ManagementError::InvalidInputFile(format!(
                "添付 {} は上限 {limit} bytes を超えています",
                attachment.filename
            )));
        }
    }
    for attachment in [definition, state] {
        if attachment.size as usize > MAX_TEXT_FILE_BYTES {
            return Err(ManagementError::InvalidInputFile(format!(
                "{} はテキスト入力上限1 MiBを超えています",
                attachment.filename
            )));
        }
    }
    for image in images {
        if !image
            .content_type
            .as_deref()
            .is_some_and(|mime| matches!(mime, "image/png" | "image/jpeg" | "image/gif" | "image/webp"))
        {
            return Err(ManagementError::InvalidInputFile(format!(
                "{} は PNG・JPEG・GIF・WebP 画像として添付してください",
                image.filename
            )));
        }
    }
    Ok(())
}

async fn download_text(attachment: &Attachment) -> Result<String, ManagementError> {
    let data = attachment.download().await.map_err(|error| {
        ManagementError::InvalidInputFile(format!("{} の取得に失敗しました: {error}", attachment.filename))
    })?;
    if data.len() > MAX_TEXT_FILE_BYTES {
        return Err(ManagementError::InvalidInputFile(format!(
            "{} は1 MiBを超えています",
            attachment.filename
        )));
    }
    String::from_utf8(data).map_err(|error| {
        ManagementError::InvalidInputFile(format!("{} は UTF-8 ではありません: {error}", attachment.filename))
    })
}

struct DiscordPreviewResponder<'a> {
    interaction: &'a CommandInteraction,
    http: &'a Http,
    uploads: &'a [PreviewUpload],
}

impl PreviewResponder for DiscordPreviewResponder<'_> {
    async fn edit_original(&self, message: &PreviewMessage) -> Result<(), String> {
        self.interaction
            .edit_response(self.http, original_response(message, self.uploads))
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    async fn create_followup(&self, message: &PreviewMessage) -> Result<(), String> {
        self.interaction
            .create_followup(self.http, followup_response(message, self.uploads))
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub(super) struct PreviewUpload {
    filename: String,
    attachment: CreateAttachment<'static>,
}

pub(super) fn prepare_uploads(images: Vec<PreviewImage>) -> Vec<PreviewUpload> {
    images
        .into_iter()
        .map(|image| PreviewUpload {
            attachment: CreateAttachment::bytes(image.data, image.filename.clone()),
            filename: image.filename,
        })
        .collect()
}

fn components(message: &PreviewMessage, images: &[PreviewUpload]) -> Vec<CreateComponent<'static>> {
    message
        .components
        .iter()
        .map(|component| match component {
            PreviewComponent::Text(text) => CreateComponent::TextDisplay(CreateTextDisplay::new(text.clone())),
            PreviewComponent::Gallery(items) => CreateComponent::MediaGallery(CreateMediaGallery::new(
                items
                    .iter()
                    .map(|item| {
                        CreateMediaGalleryItem::new(CreateUnfurledMediaItem::new(format!(
                            "attachment://{}",
                            images[item.image_index].filename
                        )))
                        .description(item.description.clone())
                    })
                    .collect::<Vec<_>>(),
            )),
        })
        .collect()
}

pub(super) fn original_response(
    message: &PreviewMessage,
    images: &[PreviewUpload],
) -> EditInteractionResponse<'static> {
    let mut reply = EditInteractionResponse::new()
        .components(components(message, images))
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .allowed_mentions(create_safe_allowed_mentions());
    for &index in &message.image_indices {
        reply = reply.new_attachment(images[index].attachment.clone());
    }
    reply
}

pub(super) fn followup_response(
    message: &PreviewMessage,
    images: &[PreviewUpload],
) -> CreateInteractionResponseFollowup<'static> {
    let mut reply = CreateInteractionResponseFollowup::new()
        .components(components(message, images))
        .flags(MessageFlags::IS_COMPONENTS_V2 | MessageFlags::EPHEMERAL)
        .allowed_mentions(create_safe_allowed_mentions());
    for &index in &message.image_indices {
        reply = reply.add_file(images[index].attachment.clone());
    }
    reply
}
