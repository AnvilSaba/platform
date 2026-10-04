use tokio::time::{Instant, timeout_at};

use super::PreviewMessage;

/// Interaction の応答境界。管理投稿や state を変更する能力は持たない。
pub trait PreviewResponder {
    async fn edit_original(&self, message: &PreviewMessage) -> Result<(), String>;
    async fn create_followup(&self, message: &PreviewMessage) -> Result<(), String>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum PreviewDeliveryStatus {
    Complete,
    DeadlineExceeded,
    Failed(String),
}

#[derive(Debug)]
pub struct PreviewDelivery {
    pub sent: usize,
    pub status: PreviewDeliveryStatus,
}

pub async fn deliver_preview(
    responder: &impl PreviewResponder,
    messages: &[PreviewMessage],
    deadline: Instant,
) -> PreviewDelivery {
    for (index, message) in messages.iter().enumerate() {
        if Instant::now() >= deadline {
            return PreviewDelivery {
                sent: index,
                status: PreviewDeliveryStatus::DeadlineExceeded,
            };
        }
        let send = async {
            if index == 0 {
                responder.edit_original(message).await
            } else {
                responder.create_followup(message).await
            }
        };
        let status = match timeout_at(deadline, send).await {
            Ok(Ok(())) => continue,
            Ok(Err(error)) => PreviewDeliveryStatus::Failed(error),
            Err(_) => PreviewDeliveryStatus::DeadlineExceeded,
        };
        return PreviewDelivery { sent: index, status };
    }
    PreviewDelivery {
        sent: messages.len(),
        status: PreviewDeliveryStatus::Complete,
    }
}
