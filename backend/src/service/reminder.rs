use super::{ServiceError, validate_description, validate_id};
use crate::{
    dao::ReminderDao,
    domain::{ReminderTask, ReminderTaskInput},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct ReminderService {
    dao: Arc<dyn ReminderDao>,
}
impl ReminderService {
    pub fn new(dao: Arc<dyn ReminderDao>) -> Self {
        Self { dao }
    }
    pub async fn list(&self) -> Result<Vec<ReminderTask>, ServiceError> {
        Ok(self.dao.list().await?)
    }
    pub async fn save(
        &self,
        id: Option<i64>,
        input: ReminderTaskInput,
    ) -> Result<ReminderTask, ServiceError> {
        if let Some(id) = id {
            validate_id(id)?;
        }
        validate_description(&input.description)?;
        if !(0..253_370_764_800_000).contains(&input.due_at) {
            return Err(ServiceError::InvalidInput(
                "date/time must be between 1970 and 9998",
            ));
        }
        if !self.dao.valid_timezone(&input.timezone).await? {
            return Err(ServiceError::InvalidInput("invalid timezone"));
        }
        self.dao
            .save(id, &input)
            .await?
            .ok_or(ServiceError::NotFound("reminder task"))
    }
    pub async fn delete(&self, id: i64) -> Result<(), ServiceError> {
        validate_id(id)?;
        if !self.dao.delete(id).await? {
            return Err(ServiceError::NotFound("reminder task"));
        }
        Ok(())
    }
    pub async fn complete(&self, id: i64, completed: bool) -> Result<ReminderTask, ServiceError> {
        validate_id(id)?;
        self.dao
            .complete(id, completed)
            .await?
            .ok_or(ServiceError::NotFound("reminder task"))
    }
    pub async fn dismiss(&self, id: i64) -> Result<(), ServiceError> {
        validate_id(id)?;
        if !self.dao.dismiss(id).await? {
            return Err(ServiceError::NotFound("daily reminder"));
        }
        Ok(())
    }
}
