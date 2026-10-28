use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::Category;

use super::{DaoError, is_foreign_key_violation};

#[async_trait]
pub trait CategoryDao: Send + Sync {
    async fn create(&self, description: &str) -> Result<Category, DaoError>;
    async fn get(&self, id: i64) -> Result<Option<Category>, DaoError>;
    async fn list(&self) -> Result<Vec<Category>, DaoError>;
    async fn update(&self, id: i64, description: &str) -> Result<Option<Category>, DaoError>;
    async fn delete(&self, id: i64) -> Result<bool, DaoError>;
}

#[derive(Clone)]
pub struct PgCategoryDao {
    pool: PgPool,
}

impl PgCategoryDao {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn category((id, description): (i64, String)) -> Category {
    Category { id, description }
}

#[async_trait]
impl CategoryDao for PgCategoryDao {
    async fn create(&self, description: &str) -> Result<Category, DaoError> {
        let row = sqlx::query_as(
            "INSERT INTO categories (description) VALUES ($1) RETURNING id, description",
        )
        .bind(description)
        .fetch_one(&self.pool)
        .await?;
        Ok(category(row))
    }

    async fn get(&self, id: i64) -> Result<Option<Category>, DaoError> {
        let row = sqlx::query_as("SELECT id, description FROM categories WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(category))
    }

    async fn list(&self) -> Result<Vec<Category>, DaoError> {
        let rows = sqlx::query_as("SELECT id, description FROM categories ORDER BY id")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(category).collect())
    }

    async fn update(&self, id: i64, description: &str) -> Result<Option<Category>, DaoError> {
        let row = sqlx::query_as(
            "UPDATE categories SET description = $2 WHERE id = $1 RETURNING id, description",
        )
        .bind(id)
        .bind(description)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(category))
    }

    async fn delete(&self, id: i64) -> Result<bool, DaoError> {
        let result = sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| {
                if is_foreign_key_violation(&error) {
                    DaoError::CategoryNotEmpty
                } else {
                    error.into()
                }
            })?;
        Ok(result.rows_affected() > 0)
    }
}
