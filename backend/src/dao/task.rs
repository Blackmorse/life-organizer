use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::{Task, TaskUrgency};

use super::{DaoError, is_foreign_key_violation};

/// Task ownership is inherited from its category; every operation is user-scoped.
#[async_trait]
pub trait TaskDao: Send + Sync {
    async fn create(
        &self,
        category_id: i64,
        description: &str,
        urgency: TaskUrgency,
    ) -> Result<Task, DaoError>;
    async fn get(&self, id: i64) -> Result<Option<Task>, DaoError>;
    async fn list(&self, category_id: Option<i64>) -> Result<Vec<Task>, DaoError>;
    async fn update(
        &self,
        id: i64,
        category_id: i64,
        description: &str,
        urgency: TaskUrgency,
    ) -> Result<Option<Task>, DaoError>;
    async fn delete(&self, id: i64) -> Result<bool, DaoError>;
}

#[derive(Clone)]
pub struct PgTaskDao {
    pool: PgPool,
    user_id: i64,
}

impl PgTaskDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self { pool, user_id }
    }
}

fn task(
    (id, category_id, description, urgency): (i64, i64, String, String),
) -> Result<Task, DaoError> {
    Ok(Task {
        id,
        category_id,
        description,
        urgency: TaskUrgency::try_from(urgency.as_str())
            .map_err(|error| sqlx::Error::Decode(error.into()))?,
    })
}

fn write_error(error: sqlx::Error) -> DaoError {
    if is_foreign_key_violation(&error) {
        DaoError::CategoryMissing
    } else {
        error.into()
    }
}

#[async_trait]
impl TaskDao for PgTaskDao {
    async fn create(
        &self,
        category_id: i64,
        description: &str,
        urgency: TaskUrgency,
    ) -> Result<Task, DaoError> {
        let row = sqlx::query_as("INSERT INTO tasks (category_id, description, urgency) SELECT id, $2, $4 FROM categories WHERE id = $1 AND user_id = $3 RETURNING id, category_id, description, urgency")
            .bind(category_id)
            .bind(description)
            .bind(self.user_id)
            .bind(urgency.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(write_error)?
            .ok_or(DaoError::CategoryMissing)?;
        task(row)
    }

    async fn get(&self, id: i64) -> Result<Option<Task>, DaoError> {
        let row = sqlx::query_as("SELECT t.id, t.category_id, t.description, t.urgency FROM tasks t JOIN categories c ON c.id = t.category_id WHERE t.id = $1 AND c.user_id = $2")
            .bind(id)
            .bind(self.user_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(task).transpose()
    }

    async fn list(&self, category_id: Option<i64>) -> Result<Vec<Task>, DaoError> {
        let rows = sqlx::query_as("SELECT t.id, t.category_id, t.description, t.urgency FROM tasks t JOIN categories c ON c.id = t.category_id WHERE ($1::bigint IS NULL OR t.category_id = $1) AND c.user_id = $2 ORDER BY t.id")
            .bind(category_id)
            .bind(self.user_id)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(task).collect()
    }

    async fn update(
        &self,
        id: i64,
        category_id: i64,
        description: &str,
        urgency: TaskUrgency,
    ) -> Result<Option<Task>, DaoError> {
        // Both the source task and destination category must belong to this user.
        // Enforce them in the write itself, not only in a preceding lookup.
        let row = sqlx::query_as("UPDATE tasks t SET category_id = $2, description = $3, urgency = $5 WHERE t.id = $1 AND EXISTS (SELECT 1 FROM categories c WHERE c.id = t.category_id AND c.user_id = $4) AND EXISTS (SELECT 1 FROM categories c WHERE c.id = $2 AND c.user_id = $4) RETURNING id, category_id, description, urgency")
            .bind(id)
            .bind(category_id)
            .bind(description)
            .bind(self.user_id)
            .bind(urgency.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(write_error)?;
        if row.is_none() && self.get(id).await?.is_some() {
            return Err(DaoError::CategoryMissing);
        }
        row.map(task).transpose()
    }

    async fn delete(&self, id: i64) -> Result<bool, DaoError> {
        let result = sqlx::query("DELETE FROM tasks t USING categories c WHERE t.id = $1 AND c.id = t.category_id AND c.user_id = $2")
            .bind(id)
            .bind(self.user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
