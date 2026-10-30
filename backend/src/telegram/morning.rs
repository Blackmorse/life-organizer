use super::TelegramSender;
use crate::{
    bot_commands::BotMessage,
    morning::{BotCallback, BotUpdate, MorningBot},
    service::{MessageSender, NotificationError},
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Updates {
    ok: bool,
    #[serde(default)]
    result: Vec<Update>,
}
#[derive(Deserialize)]
struct Update {
    update_id: i64,
    callback_query: Option<Callback>,
    message: Option<Message>,
}
#[derive(Deserialize)]
struct Callback {
    id: String,
    from: Sender,
    message: Option<Message>,
    data: Option<String>,
}
#[derive(Deserialize)]
struct Sender {
    id: i64,
    #[serde(default)]
    is_bot: bool,
}
#[derive(Deserialize)]
struct Message {
    chat: Chat,
    from: Option<Sender>,
    text: Option<String>,
}
#[derive(Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

impl TelegramSender {
    async fn call(
        &self,
        method: &str,
        body: serde_json::Value,
    ) -> Result<reqwest::Response, NotificationError> {
        self.client
            .post(format!(
                "{}{method}",
                self.endpoint.trim_end_matches("sendMessage")
            ))
            .json(&body)
            .send()
            .await
            .map_err(|_| NotificationError::DeliveryFailed)?
            .error_for_status()
            .map_err(|_| NotificationError::DeliveryFailed)
    }
    async fn call_ok(
        &self,
        method: &str,
        body: serde_json::Value,
    ) -> Result<(), NotificationError> {
        let response: super::TelegramResponse = self
            .call(method, body)
            .await?
            .json()
            .await
            .map_err(|_| NotificationError::DeliveryFailed)?;
        if response.ok {
            Ok(())
        } else {
            Err(NotificationError::DeliveryFailed)
        }
    }
}

#[async_trait]
impl MorningBot for TelegramSender {
    async fn configure_commands(&self) -> Result<(), NotificationError> {
        self.call_ok(
            "setMyCommands",
            json!({
                "scope": {"type": "all_private_chats"},
                "commands": [
                    {"command": "today", "description": "Show current Tasks of the day"},
                    {"command": "help", "description": "Show available commands"},
                    {"command": "start", "description": "Get started with Scheduler"}
                ]
            }),
        )
        .await
    }
    async fn offer(
        &self,
        recipient: i64,
        offer_id: i64,
        messages: &[String],
    ) -> Result<(), NotificationError> {
        let Some((last, earlier)) = messages.split_last() else {
            return Ok(());
        };
        self.send(recipient, earlier).await?;
        self.call_ok(
            "sendMessage",
            json!({
                "chat_id": recipient,
            "text": last,
            "parse_mode": "HTML",
                "reply_markup": {"inline_keyboard": [[
                    {"text": "Yes", "callback_data": format!("morning:{offer_id}:yes")},
                    {"text": "No", "callback_data": format!("morning:{offer_id}:no")}
                ]]}
            }),
        )
        .await
    }

    async fn updates(&self, offset: i64) -> Result<Vec<BotUpdate>, NotificationError> {
        let updates: Updates = self
            .call(
                "getUpdates",
                json!({"offset": offset, "timeout": 0, "allowed_updates": ["callback_query", "message"]}),
            )
            .await?
            .json()
            .await
            .map_err(|_| NotificationError::DeliveryFailed)?;
        if !updates.ok {
            return Err(NotificationError::DeliveryFailed);
        }
        Ok(updates
            .result
            .into_iter()
            .map(|update| BotUpdate {
                id: update.update_id,
                callback: update.callback_query.map(|callback| BotCallback {
                    id: callback.id,
                    sender: callback.from.id,
                    private_chat: callback
                        .message
                        .filter(|message| message.chat.kind == "private")
                        .map(|message| message.chat.id),
                    data: callback.data.unwrap_or_default(),
                }),
                message: update.message.and_then(|message| {
                    let from = message.from.filter(|from| !from.is_bot)?;
                    Some(BotMessage {
                        sender: from.id,
                        private_chat: (message.chat.kind == "private").then_some(message.chat.id),
                        text: message.text?,
                    })
                }),
            })
            .collect())
    }

    async fn answer(&self, callback_id: &str, text: &str) -> Result<(), NotificationError> {
        self.call_ok(
            "answerCallbackQuery",
            json!({"callback_query_id": callback_id, "text": text}),
        )
        .await
    }
}
