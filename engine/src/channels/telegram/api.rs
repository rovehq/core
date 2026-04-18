use anyhow::Result;
use serde::Serialize;

use super::types::{BotUser, GetMeResponse, GetUpdatesResponse, Message, TelegramApiResponse};
use super::TelegramBot;

impl TelegramBot {
    pub(super) async fn get_updates(&self, offset: i64) -> Result<Vec<super::types::Update>> {
        let url = format!(
            "{}?offset={}&timeout=30&allowed_updates=[\"message\",\"callback_query\"]",
            self.api_url("getUpdates"),
            offset
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await?
            .json::<GetUpdatesResponse>()
            .await?;

        if !response.ok {
            return Err(anyhow::anyhow!(
                "Telegram API getUpdates failed: {}",
                response
                    .description
                    .unwrap_or_else(|| "unknown error".to_string())
            ));
        }

        Ok(response.result.unwrap_or_default())
    }

    pub(super) async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: &str,
    ) -> Result<()> {
        let url = self.api_url("answerCallbackQuery");
        let body = serde_json::json!({
            "callback_query_id": callback_query_id,
            "text": text,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json::<TelegramApiResponse<bool>>()
            .await?;

        if !response.ok {
            return Err(anyhow::anyhow!(
                "Telegram API answerCallbackQuery failed: {}",
                response
                    .description
                    .unwrap_or_else(|| "unknown error".to_string())
            ));
        }

        Ok(())
    }

    pub(super) async fn send_message(&self, chat_id: i64, text: &str) -> Result<()> {
        self.send_message_tracked(chat_id, text).await.map(|_| ())
    }

    pub(super) async fn send_message_tracked(&self, chat_id: i64, text: &str) -> Result<Message> {
        let url = self.api_url("sendMessage");
        let scrubbed = self.secret_manager.scrub(text);

        #[derive(Serialize)]
        struct SendMsgReq<'a> {
            chat_id: i64,
            text: &'a str,
        }

        let req = SendMsgReq {
            chat_id,
            text: &scrubbed,
        };

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await?
            .json::<TelegramApiResponse<Message>>()
            .await?;

        if !response.ok {
            return Err(anyhow::anyhow!(
                "Telegram API sendMessage failed: {}",
                response
                    .description
                    .unwrap_or_else(|| "unknown error".to_string())
            ));
        }

        response
            .result
            .ok_or_else(|| anyhow::anyhow!("Telegram API sendMessage returned no message"))
    }

    pub(super) async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> Result<()> {
        let url = self.api_url("editMessageText");
        let scrubbed = self.secret_manager.scrub(text);
        let body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": scrubbed,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json::<TelegramApiResponse<serde_json::Value>>()
            .await?;

        if !response.ok {
            let description = response
                .description
                .unwrap_or_else(|| "unknown error".to_string());
            if description.contains("message is not modified") {
                return Ok(());
            }
            return Err(anyhow::anyhow!(
                "Telegram API editMessageText failed: {}",
                description
            ));
        }

        Ok(())
    }

    pub async fn get_me(&self) -> Result<BotUser> {
        let response = self
            .client
            .get(self.api_url("getMe"))
            .send()
            .await?
            .json::<GetMeResponse>()
            .await?;

        if !response.ok {
            return Err(anyhow::anyhow!(
                "Telegram API getMe failed: {}",
                response
                    .description
                    .unwrap_or_else(|| "unknown error".to_string())
            ));
        }

        response
            .result
            .ok_or_else(|| anyhow::anyhow!("Telegram API did not return bot metadata"))
    }
}
