use std::sync::Arc;

use crate::{dao::TaskDao, domain::Task};

use super::{ServiceError, validate_description, validate_id};

#[derive(Clone)]
pub struct TaskService {
    dao: Arc<dyn TaskDao>,
}

impl TaskService {
    pub fn new(dao: Arc<dyn TaskDao>) -> Self {
        Self { dao }
    }

    pub async fn create(&self, category_id: i64, description: &str) -> Result<Task, ServiceError> {
        validate_id(category_id)?;
        validate_description(description)?;
        Ok(self.dao.create(category_id, description).await?)
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
    ) -> Result<Task, ServiceError> {
        validate_id(id)?;
        validate_id(category_id)?;
        validate_description(description)?;
        self.dao
            .update(id, category_id, description)
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
}
