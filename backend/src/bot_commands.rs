//! Private-chat commands resolve live, user-scoped daily work and backlog.
use crate::{
    dao::{PgBacklogDao, PgDailyDao, PgUserDao, morning::MorningStore},
    morning::MorningBot,
    service::{BacklogService, DailyService, ServiceError, morning_messages},
};
use sqlx::PgPool;
use std::sync::Arc;

pub struct BotMessage {
    pub sender: i64,
    pub private_chat: Option<i64>,
    pub text: String,
    pub voice: Option<crate::bot_voice::VoiceMessage>,
}

pub struct BotCommands {
    pool: PgPool,
}

impl BotCommands {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn handle(
        &self,
        message: &BotMessage,
        sender: &(impl MorningBot + ?Sized),
    ) -> Result<(), ServiceError> {
        if message.sender <= 0 || message.private_chat != Some(message.sender) {
            return Ok(());
        }
        let messages = match message.text.split_whitespace().next() {
            Some("/morning") => {
                let store = MorningStore::new(self.pool.clone());
                let offers = store.unanswered(message.sender).await.map_err(crate::dao::DaoError::from)?;
                let mut sent = false;
                for offer in offers {
                    if offer.items.is_empty() {
                        continue;
                    }
                    let messages = morning_messages(&offer.items, &offer.tomorrow, &offer.yesterday);
                    sender.offer(message.sender, offer.id, &messages).await?;
                    store.sent(offer.id).await.map_err(crate::dao::DaoError::from)?;
                    sent = true;
                }
                if sent {
                    return Ok(());
                }
                vec!["No unanswered morning offer is available. Offers appear at 08:00 and expire at the next local 02:00. Use /today to check your current list or the Mini App to add tasks.".into()]
            }
            Some("/today") => {
                let user = PgUserDao::new(self.pool.clone()).get_or_create(message.sender).await?;
                let daily = DailyService::new(Arc::new(PgDailyDao::new(self.pool.clone(), user.id)));
                let messages = daily.current_messages().await?;
                if messages.is_empty() {
                    vec!["No tasks in Tasks of the day yet. Use /morning to show an unanswered morning offer, or open the Mini App to add tasks. Use /today to check again.".into()]
                } else {
                    messages
                }
            }
            Some("/remind") => {
                let user = PgUserDao::new(self.pool.clone()).get_or_create(message.sender).await?;
                let messages = BacklogService::new(Arc::new(PgBacklogDao::new(self.pool.clone(), user.id)))
                    .current_messages().await?;
                if messages.is_empty() {
                    vec!["No unfinished ordinary tasks outside Tasks of the day. Manage your plan in the Mini App.".into()]
                } else {
                    messages
                }
            }
            Some("/start" | "/help") => vec![
                "Use /today to show your current Tasks of the day, including accepted scheduled tasks. Use /morning to resend an unanswered morning offer with Yes/No buttons. Use /remind to show unfinished ordinary tasks outside today's list, most urgent first. Send a text or voice message to list today's tasks or mark a task/subtask done, for example: 'Mark buy groceries completed'. Include the full name and category if names overlap. Each message is independent. Manage tasks and progress in the Mini App.".into(),
            ],
            _ => return Ok(()),
        };
        sender.send(message.sender, &messages).await?;
        Ok(())
    }
}
