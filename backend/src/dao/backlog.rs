use super::DaoError;
use crate::domain::BacklogTask;
use async_trait::async_trait;
use sqlx::PgPool;

#[async_trait]
pub trait BacklogDao: Send + Sync {
    async fn list(&self) -> Result<Vec<BacklogTask>, DaoError>;
}

pub struct PgBacklogDao {
    pool: PgPool,
    user_id: i64,
}

impl PgBacklogDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self { pool, user_id }
    }
}

#[async_trait]
impl BacklogDao for PgBacklogDao {
    async fn list(&self) -> Result<Vec<BacklogTask>, DaoError> {
        let rows: Vec<(String, i64, String, String)> = sqlx::query_as(
            "SELECT t.description, c.id, c.description, t.urgency
             FROM tasks t JOIN categories c ON c.id=t.category_id
             WHERE c.user_id=$1 AND NOT t.completed
                  AND NOT EXISTS (SELECT 1 FROM daily_tasks d
                      WHERE d.task_id=t.id AND d.expires_at > statement_timestamp())
                  AND NOT EXISTS (SELECT 1 FROM tomorrow_tasks p
                      WHERE p.task_id=t.id AND p.expires_at > statement_timestamp())
             ORDER BY CASE t.urgency WHEN 'soon' THEN 0 WHEN 'upcoming' THEN 1 ELSE 2 END, c.id, t.id",
        )
        .bind(self.user_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(description, category_id, category, urgency)| {
                Ok(BacklogTask {
                    description,
                    category_id,
                    category,
                    urgency: crate::domain::TaskUrgency::try_from(urgency.as_str())
                        .map_err(|error| sqlx::Error::Decode(error.into()))?,
                })
            })
            .collect()
    }
}
