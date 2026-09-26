use super::ports::{LinkCodeResult, LinkCodes, LinkReply};
use crate::app::AppError;

pub async fn start_link(
    codes: &dyn LinkCodes,
    reply: &dyn LinkReply,
    user_id: u64,
    username: &str,
    server_address: &str,
) -> Result<(), AppError> {
    reply.defer_ephemeral().await?;
    let result = codes.issue(user_id, username).await;
    let content = match &result {
        Ok(LinkCodeResult::Code(code)) => format!(
            "Minecraft 26.3 で以下のサーバーに接続し、表示される入力欄にコードを入力してください。\n\nサーバーアドレス:\n```\n{server_address}\n```\nコード:\n```\n{code}\n```"
        ),
        Ok(LinkCodeResult::Blocked) => {
            "この Discordアカウントはブロックされているため、紐付けを開始できません。".into()
        }
        Err(_) => "コードを取得できませんでした。時間をおいて再度お試しください。".into(),
    };
    reply.complete(content).await?;
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::async_trait;
    use std::sync::Mutex;

    struct Codes(Result<LinkCodeResult, &'static str>);
    #[async_trait]
    impl LinkCodes for Codes {
        async fn issue(&self, _: u64, _: &str) -> Result<LinkCodeResult, AppError> {
            match &self.0 {
                Ok(LinkCodeResult::Code(code)) => Ok(LinkCodeResult::Code(code.clone())),
                Ok(LinkCodeResult::Blocked) => Ok(LinkCodeResult::Blocked),
                Err(message) => Err(anyhow::anyhow!(message.to_string())),
            }
        }
    }

    #[derive(Default)]
    struct Reply {
        deferred: Mutex<bool>,
        content: Mutex<Option<String>>,
    }
    #[async_trait]
    impl LinkReply for Reply {
        async fn defer_ephemeral(&self) -> Result<(), AppError> {
            *self.deferred.lock().unwrap() = true;
            Ok(())
        }
        async fn complete(&self, content: String) -> Result<(), AppError> {
            assert!(
                *self.deferred.lock().unwrap(),
                "response must be ephemeral before content is sent"
            );
            *self.content.lock().unwrap() = Some(content);
            Ok(())
        }
    }

    #[tokio::test]
    async fn user_receives_private_code_and_connection_instructions_without_discord() {
        let reply = Reply::default();
        start_link(
            &Codes(Ok(LinkCodeResult::Code("AC234679".into()))),
            &reply,
            42,
            "user",
            "link.example.com",
        )
        .await
        .unwrap();
        assert_eq!(
            reply.content.lock().unwrap().as_deref(),
            Some(
                "Minecraft 26.3 で以下のサーバーに接続し、表示される入力欄にコードを入力してください。\n\nサーバーアドレス:\n```\nlink.example.com\n```\nコード:\n```\nAC234679\n```"
            )
        );
    }

    #[tokio::test]
    async fn blocked_user_receives_private_refusal() {
        let reply = Reply::default();
        start_link(
            &Codes(Ok(LinkCodeResult::Blocked)),
            &reply,
            42,
            "user",
            "link.example.com",
        )
        .await
        .unwrap();
        assert_eq!(
            reply.content.lock().unwrap().as_deref(),
            Some("この Discordアカウントはブロックされているため、紐付けを開始できません。")
        );
    }

    #[tokio::test]
    async fn database_failure_receives_retry_guidance_without_internal_details() {
        let reply = Reply::default();
        let result = start_link(
            &Codes(Err("private database details")),
            &reply,
            42,
            "user",
            "link.example.com",
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            reply.content.lock().unwrap().as_deref(),
            Some("コードを取得できませんでした。時間をおいて再度お試しください。")
        );
    }
}
