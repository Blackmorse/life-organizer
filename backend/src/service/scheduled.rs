use super::{ServiceError, validate_description, validate_id};
use crate::{
    dao::ScheduledDao,
    domain::{ScheduledOccurrence, ScheduledTask, ScheduledTaskInput, Subtask},
};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone)]
pub struct ScheduledService {
    dao: Arc<dyn ScheduledDao>,
}
impl ScheduledService {
    pub fn new(dao: Arc<dyn ScheduledDao>) -> Self {
        Self { dao }
    }
    pub async fn list(&self) -> Result<Vec<ScheduledTask>, ServiceError> {
        Ok(self.dao.list().await?)
    }
    pub async fn daily(&self) -> Result<Vec<ScheduledOccurrence>, ServiceError> {
        Ok(self.dao.daily().await?)
    }
    pub async fn save(
        &self,
        id: Option<i64>,
        mut input: ScheduledTaskInput,
    ) -> Result<ScheduledTask, ServiceError> {
        if let Some(id) = id {
            validate_id(id)?;
        }
        validate_description(&input.description)?;
        if input.weekdays.is_empty()
            || input.weekdays.iter().any(|day| !(1..=7).contains(day))
            || input.weekdays.iter().collect::<HashSet<_>>().len() != input.weekdays.len()
        {
            return Err(ServiceError::InvalidInput(
                "select distinct weekdays from 1 (Monday) through 7 (Sunday)",
            ));
        }
        if !self.dao.valid_timezone(&input.timezone).await? {
            return Err(ServiceError::InvalidInput("invalid timezone"));
        }
        validate_steps(&input.subtasks)?;
        for step in &mut input.subtasks {
            step.id = None;
            step.completed = false;
        }
        input.weekdays.sort_unstable();
        self.dao
            .save(id, &input)
            .await?
            .ok_or(ServiceError::NotFound("scheduled task"))
    }
    pub async fn delete(&self, id: i64) -> Result<(), ServiceError> {
        validate_id(id)?;
        if !self.dao.delete(id).await? {
            return Err(ServiceError::NotFound("scheduled task"));
        }
        Ok(())
    }
    pub async fn progress(
        &self,
        id: i64,
        completed: Option<bool>,
        steps: Option<Vec<Subtask>>,
    ) -> Result<Vec<ScheduledOccurrence>, ServiceError> {
        validate_id(id)?;
        if let Some(steps) = &steps {
            validate_steps(steps)?;
        }
        if !self.dao.progress(id, completed, steps.as_deref()).await? {
            return Err(ServiceError::NotFound("scheduled occurrence"));
        }
        self.daily().await
    }
    pub async fn dismiss(&self, id: i64) -> Result<Vec<ScheduledOccurrence>, ServiceError> {
        validate_id(id)?;
        if !self.dao.dismiss(id).await? {
            return Err(ServiceError::NotFound("scheduled occurrence"));
        }
        self.daily().await
    }
}
fn validate_steps(steps: &[Subtask]) -> Result<(), ServiceError> {
    for step in steps {
        validate_description(&step.description)?;
        if step.id.is_some() {
            return Err(ServiceError::InvalidInput(
                "scheduled subtasks do not accept IDs",
            ));
        }
    }
    Ok(())
}
