use std::sync::Arc;

use crate::{dao::CategoryDao, domain::Category};

use super::{ServiceError, validate_description, validate_id};

#[derive(Clone)]
pub struct CategoryService {
    dao: Arc<dyn CategoryDao>,
}

impl CategoryService {
    pub fn new(dao: Arc<dyn CategoryDao>) -> Self {
        Self { dao }
    }

    pub async fn create(&self, description: &str) -> Result<Category, ServiceError> {
        validate_description(description)?;
        Ok(self.dao.create(description).await?)
    }

    pub async fn get(&self, id: i64) -> Result<Category, ServiceError> {
        validate_id(id)?;
        self.dao
            .get(id)
            .await?
            .ok_or(ServiceError::NotFound("category"))
    }

    pub async fn list(&self) -> Result<Vec<Category>, ServiceError> {
        Ok(self.dao.list().await?)
    }

    pub async fn update(&self, id: i64, description: &str) -> Result<Category, ServiceError> {
        validate_id(id)?;
        validate_description(description)?;
        self.dao
            .update(id, description)
            .await?
            .ok_or(ServiceError::NotFound("category"))
    }

    pub async fn delete(&self, id: i64) -> Result<(), ServiceError> {
        validate_id(id)?;
        if self.dao.delete(id).await? {
            Ok(())
        } else {
            Err(ServiceError::NotFound("category"))
        }
    }
}
