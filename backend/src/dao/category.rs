use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::Category;

use super::{DaoError, is_foreign_key_violation};

/// Category operations are scoped to one user for the DAO's entire lifetime.
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
    user_id: i64,
}

impl PgCategoryDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self { pool, user_id }
    }
}

fn category((id, user_id, description): (i64, i64, String)) -> Category {
    Category {
        id,
        user_id,
        description,
    }
}

#[async_trait]
impl CategoryDao for PgCategoryDao {
    async fn create(&self, description: &str) -> Result<Category, DaoError> {
        let row = sqlx::query_as(
            "INSERT INTO categories (description, user_id) VALUES ($1, $2) RETURNING id, user_id, description",
        )
        .bind(description)
        .bind(self.user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(category(row))
    }

    async fn get(&self, id: i64) -> Result<Option<Category>, DaoError> {
        let row = sqlx::query_as(
            "SELECT id, user_id, description FROM categories WHERE id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(self.user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(category))
    }

    async fn list(&self) -> Result<Vec<Category>, DaoError> {
        let rows = sqlx::query_as(
            "SELECT id, user_id, description FROM categories WHERE user_id = $1 ORDER BY id",
        )
        .bind(self.user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(category).collect())
    }

    async fn update(&self, id: i64, description: &str) -> Result<Option<Category>, DaoError> {
        let row = sqlx::query_as(
            "UPDATE categories SET description = $2 WHERE id = $1 AND user_id = $3 RETURNING id, user_id, description",
        )
        .bind(id)
        .bind(description)
        .bind(self.user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(category))
    }

    async fn delete(&self, id: i64) -> Result<bool, DaoError> {
        let result = sqlx::query("DELETE FROM categories WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(self.user_id)
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
