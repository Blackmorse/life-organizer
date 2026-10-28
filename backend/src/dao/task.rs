use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::Task;

use super::{DaoError, is_foreign_key_violation};

#[async_trait]
pub trait TaskDao: Send + Sync {
    async fn create(&self, category_id: i64, description: &str) -> Result<Task, DaoError>;
    async fn get(&self, id: i64) -> Result<Option<Task>, DaoError>;
    async fn list(&self, category_id: Option<i64>) -> Result<Vec<Task>, DaoError>;
    async fn update(
        &self,
        id: i64,
        category_id: i64,
        description: &str,
    ) -> Result<Option<Task>, DaoError>;
    async fn delete(&self, id: i64) -> Result<bool, DaoError>;
}

#[derive(Clone)]
pub struct PgTaskDao {
    pool: PgPool,
}

impl PgTaskDao {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn task((id, category_id, description): (i64, i64, String)) -> Task {
    Task {
        id,
        category_id,
        description,
    }
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
    async fn create(&self, category_id: i64, description: &str) -> Result<Task, DaoError> {
        let row = sqlx::query_as("INSERT INTO tasks (category_id, description) VALUES ($1, $2) RETURNING id, category_id, description")
            .bind(category_id)
            .bind(description)
            .fetch_one(&self.pool)
            .await
            .map_err(write_error)?;
        Ok(task(row))
    }

    async fn get(&self, id: i64) -> Result<Option<Task>, DaoError> {
        let row = sqlx::query_as("SELECT id, category_id, description FROM tasks WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(task))
    }

    async fn list(&self, category_id: Option<i64>) -> Result<Vec<Task>, DaoError> {
        let rows = sqlx::query_as("SELECT id, category_id, description FROM tasks WHERE ($1::bigint IS NULL OR category_id = $1) ORDER BY id")
            .bind(category_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(task).collect())
    }

    async fn update(
        &self,
        id: i64,
        category_id: i64,
        description: &str,
    ) -> Result<Option<Task>, DaoError> {
        let row = sqlx::query_as("UPDATE tasks SET category_id = $2, description = $3 WHERE id = $1 RETURNING id, category_id, description")
            .bind(id)
            .bind(category_id)
            .bind(description)
            .fetch_optional(&self.pool)
            .await
            .map_err(write_error)?;
        Ok(row.map(task))
    }

    async fn delete(&self, id: i64) -> Result<bool, DaoError> {
        let result = sqlx::query("DELETE FROM tasks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
