use super::ServiceError;
use crate::dao::daily_work::{DailyMove, DailyWork, DailyWorkDao};
use std::sync::Arc;

/// Shared application boundary for daily-list tools, independent of LLM/Telegram.
#[derive(Clone)]
pub struct DailyWorkService {
    dao: Arc<dyn DailyWorkDao>,
}

impl DailyWorkService {
    pub fn new(dao: Arc<dyn DailyWorkDao>) -> Self {
        Self { dao }
    }

    pub async fn list(&self) -> Result<Vec<DailyWork>, ServiceError> {
        Ok(self.dao.list().await?)
    }

    pub async fn move_to_date(
        &self,
        listed: &DailyWork,
        date: &str,
        timezone: Option<&str>,
    ) -> Result<DailyMove, ServiceError> {
        if listed.completed {
            return Err(ServiceError::InvalidInput("completed work cannot be moved"));
        }
        self.dao
            .move_to_date(listed, date, timezone)
            .await?
            .ok_or(ServiceError::NotFound(
                "current daily item (reload today's list)",
            ))
    }

    pub async fn complete(&self, listed: &DailyWork) -> Result<(), ServiceError> {
        if !self.dao.complete(listed).await? {
            return Err(ServiceError::NotFound(
                "current daily item (reload today's list)",
            ));
        }
        Ok(())
    }
}
