//! Outbound Telegram transport. Never expose/log URLs: they contain the bot token.
mod morning;
mod voice;
use crate::service::{MessageSender, NotificationError};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

pub struct TelegramSender {
    client: reqwest::Client,
    endpoint: String,
    bot_id: i64,
}

impl TelegramSender {
    pub fn new(token: &str) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint: format!("https://api.telegram.org/bot{token}/sendMessage"),
            bot_id: token
                .split(':')
                .next()
                .and_then(|id| id.parse().ok())
                .unwrap_or(0),
        })
    }
}

#[derive(Deserialize)]
struct TelegramResponse {
    ok: bool,
    error_code: Option<u16>,
    description: Option<String>,
}

#[async_trait]
impl MessageSender for TelegramSender {
    async fn send(&self, recipient: i64, messages: &[String]) -> Result<(), NotificationError> {
        for (index, text) in messages.iter().enumerate() {
            let response = self
                .client
                .post(&self.endpoint)
                .json(
                    &serde_json::json!({"chat_id": recipient, "text": text, "parse_mode": "HTML"}),
                )
                .send()
                .await
                .map_err(|_| NotificationError::DeliveryFailed)?;
            let status = response.status();
            let payload: TelegramResponse = response
                .json()
                .await
                .map_err(|_| NotificationError::DeliveryFailed)?;
            if !status.is_success() || !payload.ok {
                // A first-message 403 means blocked bot/no private-chat permission.
                // Any later failure may follow a successful partial delivery.
                return Err(
                    if index == 0
                        && (status.as_u16() == 403
                            || payload.error_code == Some(403)
                            || (payload.error_code == Some(400)
                                && payload.description.as_deref()
                                    == Some("Bad Request: chat not found")))
                    {
                        NotificationError::ChatUnavailable
                    } else {
                        NotificationError::DeliveryFailed
                    },
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
    use serde_json::{Value, json};
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    #[test]
    fn polling_identity_uses_bot_id_and_survives_token_rotation() {
        use crate::morning::MorningBot;
        assert_eq!(
            TelegramSender::new("123:first-secret")
                .unwrap()
                .polling_id(),
            123
        );
        assert_eq!(
            TelegramSender::new("123:rotated-secret")
                .unwrap()
                .polling_id(),
            123
        );
        assert_eq!(
            TelegramSender::new("456:other-secret")
                .unwrap()
                .polling_id(),
            456
        );
    }

    #[derive(Clone)]
    struct FakeState {
        replies: Arc<Mutex<VecDeque<(StatusCode, Value)>>>,
        requests: Arc<Mutex<Vec<Value>>>,
    }
    struct FakeBot {
        sender: TelegramSender,
        state: FakeState,
        server: tokio::task::JoinHandle<()>,
    }
    impl Drop for FakeBot {
        fn drop(&mut self) {
            self.server.abort();
        }
    }
    impl FakeBot {
        async fn new(replies: Vec<(StatusCode, Value)>) -> Self {
            let state = FakeState {
                replies: Arc::new(Mutex::new(replies.into())),
                requests: Arc::new(Mutex::new(Vec::new())),
            };
            let app = Router::new()
                .route(
                    "/botunit-test-token/{method}",
                    post(
                        |State(state): State<FakeState>, Json(body): Json<Value>| async move {
                            state.requests.lock().unwrap().push(body);
                            let (status, body) = state
                                .replies
                                .lock()
                                .unwrap()
                                .pop_front()
                                .expect("unexpected Telegram retry");
                            (status, Json(body))
                        },
                    ),
                )
                .with_state(state.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let mut sender = TelegramSender::new("unit-test-token").unwrap();
            sender.endpoint = format!("http://{address}/botunit-test-token/sendMessage");
            Self {
                sender,
                state,
                server,
            }
        }
    }

    #[tokio::test]
    async fn morning_offer_attaches_choices_and_polls_private_callbacks() {
        use crate::morning::MorningBot;
        let bot = FakeBot::new(vec![
            (StatusCode::OK, json!({"ok":true})),
            (StatusCode::OK, json!({"ok":true})),
            (StatusCode::OK, json!({"ok":true,"result":[
                {"update_id":9,"callback_query":{"id":"cb","from":{"id":123},"message":{"chat":{"id":123,"type":"private"}},"data":"morning:42:yes"}},
                {"update_id":10,"callback_query":{"id":"group","from":{"id":123},"message":{"chat":{"id":-1,"type":"group"}},"data":"morning:42:no"}},
                {"update_id":11,"message":{"chat":{"id":123,"type":"private"},"from":{"id":123},"text":"unrelated"}}
            ]})),
            (StatusCode::OK, json!({"ok":true})),
        ]).await;
        bot.sender
            .offer(
                123,
                42,
                &["<b>First</b>".into(), "<b>Last</b>\nAdd these?".into()],
            )
            .await
            .unwrap();
        let updates = bot.sender.updates(9).await.unwrap();
        assert_eq!(updates.len(), 3);
        assert_eq!(updates[0].id, 9);
        let callback = updates[0].callback.as_ref().unwrap();
        assert_eq!(callback.sender, 123);
        assert_eq!(callback.private_chat, Some(123));
        assert_eq!(callback.data, "morning:42:yes");
        assert_eq!(updates[1].callback.as_ref().unwrap().private_chat, None);
        assert!(updates[2].callback.is_none());
        bot.sender.answer("cb", "Added").await.unwrap();
        let requests = bot.state.requests.lock().unwrap();
        assert_eq!(
            requests[0],
            json!({"chat_id":123,"text":"<b>First</b>","parse_mode":"HTML"})
        );
        assert_eq!(
            requests[1],
            json!({"chat_id":123,"text":"<b>Last</b>\nAdd these?","parse_mode":"HTML","reply_markup":{"inline_keyboard":[[
                {"text":"Yes","callback_data":"morning:42:yes"},
                {"text":"No","callback_data":"morning:42:no"}
            ]]}})
        );
        assert_eq!(
            requests[2],
            json!({"offset":9,"timeout":0,"allowed_updates":["callback_query","message"]})
        );
        assert_eq!(
            requests[3],
            json!({"callback_query_id":"cb","text":"Added"})
        );
    }

    #[tokio::test]
    async fn registers_private_chat_commands_and_parses_message_identity() {
        use crate::morning::MorningBot;
        let bot = FakeBot::new(vec![
            (StatusCode::OK, json!({"ok":true})),
            (StatusCode::OK, json!({"ok":true,"result":[
                {"update_id":1,"message":{"chat":{"id":123,"type":"private"},"from":{"id":123},"text":"/today"}},
                {"update_id":2,"message":{"chat":{"id":-1,"type":"group"},"from":{"id":123},"text":"/today"}},
                {"update_id":3,"message":{"chat":{"id":123,"type":"private"},"from":{"id":456},"text":"/today"}},
                {"update_id":4,"message":{"chat":{"id":123,"type":"private"},"from":{"id":123,"is_bot":true},"text":"/today"}},
                {"update_id":5,"message":{"chat":{"id":123,"type":"private"},"text":"/today"}},
                {"update_id":6,"message":{"chat":{"id":123,"type":"private"},"from":{"id":123},"photo":[]}},
                {"update_id":7,"edited_message":{"chat":{"id":123,"type":"private"},"from":{"id":123},"text":"/today"}},
                {"update_id":8,"message":{"chat":{"id":123,"type":"private"},"from":{"id":123},"voice":{"file_id":"voice-id","file_size":42,"duration":2}}},
                {"update_id":9,"message":{"chat":{"id":123,"type":"private"},"from":{"id":123,"is_bot":true},"voice":{"file_id":"ignored"}}}
            ]})),
        ]).await;
        bot.sender.configure_commands().await.unwrap();
        let updates = bot.sender.updates(0).await.unwrap();
        let message = updates[0].message.as_ref().unwrap();
        assert_eq!(message.sender, 123);
        assert_eq!(message.private_chat, Some(123));
        assert_eq!(message.text, "/today");
        assert_eq!(updates[1].message.as_ref().unwrap().private_chat, None);
        assert_eq!(updates[2].message.as_ref().unwrap().sender, 456);
        assert_eq!(updates[2].message.as_ref().unwrap().private_chat, Some(123));
        assert!(updates[3..7].iter().all(|update| update.message.is_none()));
        let voice = updates[7].message.as_ref().unwrap();
        assert_eq!(voice.sender, 123);
        assert_eq!(voice.private_chat, Some(123));
        assert!(voice.text.is_empty());
        assert_eq!(voice.voice.as_ref().unwrap().file_id, "voice-id");
        assert_eq!(voice.voice.as_ref().unwrap().file_size, Some(42));
        assert!(updates[8].message.is_none());
        let requests = bot.state.requests.lock().unwrap();
        assert_eq!(
            requests[0],
            json!({
                "scope":{"type":"all_private_chats"},
                "commands":[
                    {"command":"today","description":"Show current Tasks of the day"},
                    {"command":"remind","description":"Show unfinished tasks outside today's list"},
                    {"command":"help","description":"Show available commands"},
                    {"command":"start","description":"Get started with Scheduler"}
                ]
            })
        );
    }

    #[tokio::test]
    async fn sends_html_chunks_in_order_to_private_chat() {
        let bot = FakeBot::new(vec![(StatusCode::OK, json!({"ok":true})); 2]).await;
        bot.sender
            .send(123, &["<b>🔹 Today</b>".into(), "Second".into()])
            .await
            .unwrap();
        assert_eq!(
            *bot.state.requests.lock().unwrap(),
            vec![
                json!({"chat_id":123,"text":"<b>🔹 Today</b>", "parse_mode":"HTML"}),
                json!({"chat_id":123,"text":"Second", "parse_mode":"HTML"}),
            ]
        );
    }

    #[tokio::test]
    async fn rejects_api_http_and_malformed_failures_without_retry_or_secret_details() {
        for (status, payload, unavailable) in [
            (
                StatusCode::FORBIDDEN,
                json!({"ok":false,"error_code":403}),
                true,
            ),
            (
                StatusCode::BAD_REQUEST,
                json!({"ok":false,"error_code":400,"description":"Bad Request: chat not found"}),
                true,
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                json!({"ok":false,"error_code":429}),
                false,
            ),
            (
                StatusCode::OK,
                json!({"ok":false,"description":"unit-test-token"}),
                false,
            ),
            (StatusCode::INTERNAL_SERVER_ERROR, json!({"ok":true}), false),
            (StatusCode::OK, json!({"invalid":"unit-test-token"}), false),
        ] {
            let bot = FakeBot::new(vec![(status, payload)]).await;
            let error = bot.sender.send(123, &["Summary".into()]).await.unwrap_err();
            assert_eq!(
                matches!(error, NotificationError::ChatUnavailable),
                unavailable
            );
            assert!(!format!("{error:?}: {error}").contains("unit-test-token"));
            assert_eq!(bot.state.requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn stops_after_partial_delivery_and_warns_before_retry() {
        let bot = FakeBot::new(vec![
            (StatusCode::OK, json!({"ok":true})),
            (StatusCode::FORBIDDEN, json!({"ok":false,"error_code":403})),
        ])
        .await;
        let error = bot
            .sender
            .send(123, &["First".into(), "Second".into(), "Third".into()])
            .await
            .unwrap_err();
        assert!(matches!(error, NotificationError::DeliveryFailed));
        assert_eq!(bot.state.requests.lock().unwrap().len(), 2);
    }
}
