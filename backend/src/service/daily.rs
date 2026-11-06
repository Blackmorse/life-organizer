use super::{MessageSender, ServiceError, notification::daily_messages, validate_id};
use crate::{dao::DailyDao, domain::DailyTask};
use std::sync::Arc;

pub struct DailyPlanningService {
    dao: Arc<dyn crate::dao::DailyPlanningDao>,
}

impl DailyPlanningService {
    pub fn new(dao: Arc<dyn crate::dao::DailyPlanningDao>) -> Self {
        Self { dao }
    }

    pub async fn plan(
        &self,
        listed: &crate::domain::Task,
        index: Option<usize>,
        date: &str,
        timezone: Option<&str>,
    ) -> Result<crate::dao::daily_work::DailyMove, ServiceError> {
        validate_id(listed.id)?;
        if index.is_some_and(|index| index >= listed.subtasks.len()) {
            return Err(ServiceError::NotFound("subtask"));
        }
        self.dao
            .plan(listed, index, date, timezone)
            .await?
            .ok_or(ServiceError::NotFound("task or subtask"))
    }
}

#[derive(Clone)]
pub struct DailyService {
    dao: Arc<dyn DailyDao>,
}
impl DailyService {
    pub async fn finalize(
        &self,
        recipient: i64,
        sender: &dyn MessageSender,
    ) -> Result<(), ServiceError> {
        let messages = self.current_messages().await?;
        if messages.is_empty() {
            return Err(ServiceError::InvalidInput(
                "Add tasks or subtasks to Tasks of the day before finalizing.",
            ));
        }
        sender.send(recipient, &messages).await?;
        Ok(())
    }
    pub async fn current_messages(&self) -> Result<Vec<String>, ServiceError> {
        let items = self.dao.summary().await?;
        Ok(if items.is_empty() {
            Vec::new()
        } else {
            daily_messages(&items)
        })
    }
    pub fn new(dao: Arc<dyn DailyDao>) -> Self {
        Self { dao }
    }
    pub async fn list(&self) -> Result<Vec<DailyTask>, ServiceError> {
        Ok(self.dao.list().await?)
    }
    pub async fn select(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: &str,
    ) -> Result<Vec<DailyTask>, ServiceError> {
        validate_id(task_id)?;
        if !self.dao.valid_timezone(timezone).await? {
            return Err(ServiceError::InvalidInput("invalid timezone"));
        }
        if !self.dao.select(task_id, index, timezone).await? {
            return Err(ServiceError::NotFound("task or subtask"));
        }
        self.list().await
    }
    pub async fn remove(
        &self,
        task_id: i64,
        subtask_id: &str,
    ) -> Result<Vec<DailyTask>, ServiceError> {
        validate_id(task_id)?;
        self.dao.remove(task_id, subtask_id).await?;
        self.list().await
    }

    pub async fn select_date(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: &str,
        date: &str,
    ) -> Result<Vec<DailyTask>, ServiceError> {
        validate_id(task_id)?;
        if !self.dao.valid_timezone(timezone).await? {
            return Err(ServiceError::InvalidInput("invalid timezone"));
        }
        if !self.dao.select_date(task_id, index, timezone, date).await? {
            return Err(ServiceError::NotFound("task or subtask"));
        }
        self.list().await
    }
}
