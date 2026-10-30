//! Private-chat commands resolve live, user-scoped daily work and backlog.
use crate::{
    dao::{PgBacklogDao, PgDailyDao, PgUserDao},
    service::{BacklogService, DailyService, MessageSender, ServiceError},
};
use sqlx::PgPool;
use std::sync::Arc;

pub struct BotMessage {
    pub sender: i64,
    pub private_chat: Option<i64>,
    pub text: String,
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
        sender: &(impl MessageSender + ?Sized),
    ) -> Result<(), ServiceError> {
        if message.sender <= 0 || message.private_chat != Some(message.sender) {
            return Ok(());
        }
        let messages = match message.text.split_whitespace().next() {
            Some("/today") => {
                let user = PgUserDao::new(self.pool.clone()).get_or_create(message.sender).await?;
                let daily = DailyService::new(Arc::new(PgDailyDao::new(self.pool.clone(), user.id)));
                let messages = daily.current_messages().await?;
                if messages.is_empty() {
                    vec!["No tasks in Tasks of the day yet. Open the Mini App to add tasks, or accept a morning offer. Use /today to check again.".into()]
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
                "Use /today to show your current Tasks of the day, including accepted scheduled tasks. Use /remind to show unfinished ordinary tasks outside today's list, most urgent first. Manage tasks and progress in the Mini App.".into(),
            ],
            _ => return Ok(()),
        };
        sender.send(message.sender, &messages).await?;
        Ok(())
    }
}
