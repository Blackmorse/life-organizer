use std::sync::Arc;

use crate::{
    dao::TaskDao,
    domain::{Subtask, Task, TaskUrgency},
};

use super::{ServiceError, validate_description, validate_id};

#[derive(Clone)]
pub struct TaskService {
    dao: Arc<dyn TaskDao>,
}

impl TaskService {
    pub fn new(dao: Arc<dyn TaskDao>) -> Self {
        Self { dao }
    }

    pub async fn create(
        &self,
        category_id: i64,
        description: &str,
        urgency: TaskUrgency,
    ) -> Result<Task, ServiceError> {
        validate_id(category_id)?;
        validate_description(description)?;
        Ok(self.dao.create(category_id, description, urgency).await?)
    }

    /// Voice creation path: task and its initial steps land in one atomic write.
    pub async fn create_with_subtasks(
        &self,
        category_id: i64,
        description: &str,
        urgency: TaskUrgency,
        subtasks: &[Subtask],
    ) -> Result<Task, ServiceError> {
        validate_id(category_id)?;
        validate_description(description)?;
        for subtask in subtasks {
            validate_description(&subtask.description)?;
        }
        Ok(self
            .dao
            .create_with_subtasks(category_id, description, urgency, subtasks)
            .await?)
    }

    /// Appends one unchecked step only while the listed task identity still matches.
    pub async fn append_subtask(
        &self,
        listed: &Task,
        description: &str,
    ) -> Result<Task, ServiceError> {
        validate_id(listed.id)?;
        validate_description(description)?;
        self.dao
            .append_subtask(listed, description)
            .await?
            .ok_or(ServiceError::NotFound("task"))
    }

    pub async fn get(&self, id: i64) -> Result<Task, ServiceError> {
        validate_id(id)?;
        self.dao
            .get(id)
            .await?
            .ok_or(ServiceError::NotFound("task"))
    }

    pub async fn list(&self, category_id: Option<i64>) -> Result<Vec<Task>, ServiceError> {
        if let Some(id) = category_id {
            validate_id(id)?;
        }
        Ok(self.dao.list(category_id).await?)
    }

    pub async fn update(
        &self,
        id: i64,
        category_id: i64,
        description: &str,
        urgency: TaskUrgency,
    ) -> Result<Task, ServiceError> {
        validate_id(id)?;
        validate_id(category_id)?;
        validate_description(description)?;
        self.dao
            .update(id, category_id, description, urgency)
            .await?
            .ok_or(ServiceError::NotFound("task"))
    }

    pub async fn delete(&self, id: i64) -> Result<(), ServiceError> {
        validate_id(id)?;
        if self.dao.delete(id).await? {
            Ok(())
        } else {
            Err(ServiceError::NotFound("task"))
        }
    }

    pub async fn set_completed(&self, id: i64, completed: bool) -> Result<Task, ServiceError> {
        validate_id(id)?;
        self.dao
            .set_completed(id, completed)
            .await?
            .ok_or(ServiceError::NotFound("task"))
    }

    pub async fn replace_subtasks(
        &self,
        id: i64,
        subtasks: &[Subtask],
    ) -> Result<Task, ServiceError> {
        validate_id(id)?;
        for subtask in subtasks {
            validate_description(&subtask.description)?;
        }
        self.dao
            .replace_subtasks(id, subtasks)
            .await?
            .ok_or(ServiceError::NotFound("task"))
    }
}
