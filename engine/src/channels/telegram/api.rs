use anyhow::Result;
use serde::Serialize;

use super::types::{BotUser, GetMeResponse, GetUpdatesResponse};
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
            return Err(anyhow::anyhow!("Telegram API returned ok=false"));
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
        self.client.post(&url).json(&body).send().await?;
        Ok(())
    }

    pub async fn send_message(&self, chat_id: i64, text: &str) -> Result<()> {
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

        self.client.post(&url).json(&req).send().await?;
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
            return Err(anyhow::anyhow!("Telegram API returned ok=false"));
        }

        response
            .result
            .ok_or_else(|| anyhow::anyhow!("Telegram API did not return bot metadata"))
    }
}
