use anyhow::Result;
use serde::Serialize;

use super::types::GetUpdatesResponse;
use super::TelegramBot;

impl TelegramBot {
    pub(super) async fn get_updates(&self, offset: i64) -> Result<Vec<super::types::Update>> {
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30&allowed_updates=[\"message\",\"callback_query\"]",
            self.token, offset
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
        let url = format!(
            "https://api.telegram.org/bot{}/answerCallbackQuery",
            self.token
        );
        let body = serde_json::json!({
            "callback_query_id": callback_query_id,
            "text": text,
        });
        self.client.post(&url).json(&body).send().await?;
        Ok(())
    }

    pub async fn send_message(&self, chat_id: i64, text: &str) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
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
}
