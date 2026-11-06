//! Durable morning-offer orchestration. Telegram transport and SQL stay behind
//! their own boundaries; callbacks never trust a client-supplied owner.
use crate::{
    bot_commands::{BotCommands, BotMessage},
    dao::{
        PgBacklogDao,
        morning::{Decision, MorningStore},
    },
    service::{BacklogService, MessageSender, NotificationError, ServiceError, morning_messages},
    speech::{SpeechError, SpeechRecognizer},
    task_assistant::VoiceAssistant,
};
use async_trait::async_trait;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};

pub struct BotUpdate {
    pub id: i64,
    pub callback: Option<BotCallback>,
    pub message: Option<BotMessage>,
}

pub struct BotCallback {
    pub id: String,
    pub sender: i64,
    pub private_chat: Option<i64>,
    pub data: String,
}

#[async_trait]
pub trait MorningBot: MessageSender {
    fn polling_id(&self) -> i64 {
        0
    }

    async fn download_voice(&self, _file_id: &str) -> Result<Vec<u8>, SpeechError> {
        Err(SpeechError::RecognitionFailed)
    }
    async fn configure_commands(&self) -> Result<(), NotificationError>;
    async fn offer(
        &self,
        recipient: i64,
        offer_id: i64,
        messages: &[String],
    ) -> Result<(), NotificationError>;
    async fn updates(&self, offset: i64) -> Result<Vec<BotUpdate>, NotificationError>;
    async fn answer(&self, callback_id: &str, text: &str) -> Result<(), NotificationError>;
}

pub struct MorningWorker {
    pool: PgPool,
    store: MorningStore,
    commands: BotCommands,
    bot: Arc<dyn MorningBot>,
    speech: Option<Arc<dyn SpeechRecognizer>>,
    assistant: Option<Arc<dyn VoiceAssistant>>,
    backlog_delivery: tokio::sync::Mutex<()>,
}

impl MorningWorker {
    pub fn new(pool: PgPool, bot: Arc<dyn MorningBot>) -> Self {
        Self {
            pool: pool.clone(),
            store: MorningStore::new(pool.clone()).for_bot(bot.polling_id()),
            commands: BotCommands::new(pool),
            bot,
            speech: None,
            assistant: None,
            backlog_delivery: tokio::sync::Mutex::new(()),
        }
    }

    pub fn with_speech(mut self, speech: Arc<dyn SpeechRecognizer>) -> Self {
        self.speech = Some(speech);
        self
    }

    pub fn with_assistant(mut self, assistant: Arc<dyn VoiceAssistant>) -> Self {
        self.assistant = Some(assistant);
        self
    }

    pub async fn deliver_due(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        crate::dao::generate_scheduled(&self.pool, None).await?;
        self.store.sweep().await?;
        // Bound each batch so callbacks remain responsive even with a backlog.
        for _ in 0..20 {
            let Some(offer) = self.store.claim().await? else {
                break;
            };
            let messages = morning_messages(&offer.items, &offer.tomorrow, &offer.yesterday);
            if !offer.items.is_empty() {
                self.bot.offer(offer.recipient, offer.id, &messages).await?;
            } else if !messages.is_empty() {
                self.bot.send(offer.recipient, &messages).await?;
            }
            if offer.items.is_empty() {
                self.store.ready_without_choice(offer.id).await?;
            } else {
                self.store.sent(offer.id).await?;
            }
        }
        self.deliver_backlogs().await?;
        Ok(())
    }

    async fn deliver_backlogs(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Polling and timer delivery run concurrently in the elected worker.
        // Serialize follow-ups so multiple timezone groups cannot race a send.
        let _delivery = self.backlog_delivery.lock().await;
        for _ in 0..20 {
            let Some((id, user_id, recipient)) = self.store.claim_backlog().await? else {
                break;
            };
            let messages =
                BacklogService::new(Arc::new(PgBacklogDao::new(self.pool.clone(), user_id)))
                    .current_messages()
                    .await?;
            if !messages.is_empty() {
                self.bot.send(recipient, &messages).await?;
            }
            self.store.backlog_sent(id).await?;
        }
        Ok(())
    }

    pub async fn poll(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut offset = self.store.offset().await?;
        for update in self.bot.updates(offset).await? {
            if update.id < offset {
                continue;
            }
            if let Some(callback) = update.callback {
                let text = if let Some((id, yes)) = parse_choice(&callback.data)
                    .filter(|_| callback.private_chat == Some(callback.sender))
                {
                    match self.store.decide(id, callback.sender, yes).await? {
                        Decision::Accepted(_) => {
                            "Added tasks to Tasks of the day. Open or refresh the Mini App to see them."
                        }
                        Decision::Declined => "No tasks added.",
                        Decision::Unavailable => "This offer has expired or was already answered.",
                    }
                } else {
                    "This button is unavailable."
                };
                // The choice is already durable/idempotent. A failed callback
                // acknowledgement must not block later updates indefinitely.
                let _ = self.bot.answer(&callback.id, text).await;
            }
            // Persist before command/text/voice side effects. If storage fails, do not
            // send a reply that the next poll would send again. Callback choices
            // above are durable and idempotent if this write needs a retry.
            offset = update.id + 1;
            self.store.advance(offset).await?;
            if let Some(message) = update.message.as_ref()
                && message.voice.is_some()
                && let Some(speech) = &self.speech
                && crate::bot_voice::handle(
                    message,
                    self.bot.as_ref(),
                    speech.as_ref(),
                    self.assistant.as_deref(),
                )
                .await
                .is_err()
            {
                eprintln!("Voice reply delivery failed; the user can send the recording again.");
            }
            if let Some(message) = update.message.as_ref()
                && message.voice.is_none()
                && let Some(assistant) = self.assistant.as_deref()
                && crate::bot_voice::handle_text(message, self.bot.as_ref(), assistant)
                    .await
                    .is_err()
            {
                eprintln!("Text reply delivery failed; the user can check /today before retrying.");
            }
            if let Some(message) = update.message
                && message.voice.is_none()
                && let Err(error) = self.commands.handle(&message, self.bot.as_ref()).await
            {
                // Do not replay a possibly partially delivered summary or let
                // one failed command block later commands and callbacks.
                if !matches!(error, ServiceError::Notification(_)) {
                    let _ = self
                        .bot
                        .send(
                            message.sender,
                            &["Could not load tasks. Please retry your command.".into()],
                        )
                        .await;
                }
                eprintln!("Bot command failed; the user can retry the command.");
            }
        }
        // Decisions are committed and offsets advanced before follow-up delivery.
        // A failed reminder retries via its lease rather than replaying a callback.
        self.deliver_backlogs().await?;
        Ok(())
    }
}

fn parse_choice(data: &str) -> Option<(i64, bool)> {
    let mut parts = data.split(':');
    if parts.next()? != "morning" {
        return None;
    }
    let id = parts.next()?.parse::<i64>().ok().filter(|id| *id > 0)?;
    let yes = match parts.next()? {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((id, yes))
}

/// One active poller per database. A dedicated PostgreSQL session holds the lock
/// and is explicitly closed on shutdown (never returned to the pool locked).
pub async fn run(
    pool: PgPool,
    bot: Arc<dyn MorningBot>,
    speech: Arc<dyn SpeechRecognizer>,
    assistant: Arc<dyn VoiceAssistant>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let worker = MorningWorker::new(pool.clone(), bot)
        .with_speech(speech)
        .with_assistant(assistant);
    loop {
        if *stop.borrow() {
            return;
        }
        let connection = pool.acquire().await;
        if let Ok(mut connection) = connection {
            let leader = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock(734891302)")
                .fetch_one(&mut *connection)
                .await
                .unwrap_or(false);
            if leader {
                if worker.bot.configure_commands().await.is_err() {
                    eprintln!(
                        "Bot command menu setup failed; commands remain available by typing them."
                    );
                }
                let polling = async {
                    let mut interval = tokio::time::interval(Duration::from_secs(2));
                    loop {
                        interval.tick().await;
                        if sqlx::query("SELECT 1")
                            .execute(&mut *connection)
                            .await
                            .is_err()
                        {
                            break;
                        }
                        if worker.poll().await.is_err() {
                            eprintln!("Telegram update polling failed; will retry.");
                        }
                    }
                };
                let delivery = async {
                    let mut sweeps = tokio::time::interval(Duration::from_secs(30));
                    loop {
                        sweeps.tick().await;
                        if worker.deliver_due().await.is_err() {
                            eprintln!("Morning offer delivery failed; will retry pending offers.");
                        }
                    }
                };
                tokio::select! {
                    _ = stop.changed() => {},
                    _ = polling => {},
                    _ = delivery => {},
                }
            }
            let _ = connection.close().await;
        }
        if *stop.borrow() {
            return;
        }
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_exact_positive_offer_choices() {
        assert_eq!(parse_choice("morning:42:yes"), Some((42, true)));
        assert_eq!(parse_choice("morning:42:no"), Some((42, false)));
        for bad in [
            "morning:0:yes",
            "morning:-1:no",
            "morning:1:yes:extra",
            "other:1:yes",
            "morning:1:maybe",
        ] {
            assert_eq!(parse_choice(bad), None);
        }
    }
}
