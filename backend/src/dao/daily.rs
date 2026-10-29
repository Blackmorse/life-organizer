use super::DaoError;
use crate::domain::{DailyTask, DailyTaskSummary, Subtask};
use async_trait::async_trait;
use sqlx::{PgPool, types::Json};

#[async_trait]
pub trait DailyDao: Send + Sync {
    async fn summary(&self) -> Result<Vec<DailyTaskSummary>, DaoError>;
    async fn list(&self) -> Result<Vec<DailyTask>, DaoError>;
    async fn select(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: &str,
    ) -> Result<bool, DaoError>;
    async fn remove(&self, task_id: i64, subtask_id: &str) -> Result<(), DaoError>;
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError>;
}

pub struct PgDailyDao {
    pool: PgPool,
    user_id: i64,
}
impl PgDailyDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self { pool, user_id }
    }
}

#[async_trait]
impl DailyDao for PgDailyDao {
    async fn summary(&self) -> Result<Vec<DailyTaskSummary>, DaoError> {
        super::generate_scheduled(&self.pool, Some(self.user_id)).await?;
        let rows: Vec<(i64, String, String, Option<String>, bool, i64)> = sqlx::query_as(
            "SELECT t.id, c.description, t.description, s.step->>'description',
                t.completed OR COALESCE((s.step->>'completed')::boolean, false), COALESCE(s.position, 0)
             FROM daily_tasks d
             JOIN tasks t ON t.id = d.task_id
             JOIN categories c ON c.id = t.category_id
             LEFT JOIN LATERAL (
                 SELECT step, position FROM jsonb_array_elements(t.subtasks)
                 WITH ORDINALITY AS steps(step, position)
                 WHERE step->>'id' = d.subtask_id
             ) s ON true
             WHERE c.user_id = $1 AND d.expires_at > statement_timestamp()
                 AND (d.subtask_id = '' OR s.step IS NOT NULL)
             UNION ALL
             SELECT -o.id, '', o.description, NULL, o.completed, 0::bigint
             FROM scheduled_occurrences o JOIN scheduled_tasks st ON st.id=o.scheduled_task_id
             WHERE st.user_id=$1 AND NOT o.hidden AND o.expires_at > statement_timestamp()
             ORDER BY 1, 6",
        )
        .bind(self.user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(task_id, category, task, subtask, completed, _)| DailyTaskSummary {
                    task_id,
                    category,
                    task,
                    subtask,
                    completed,
                },
            )
            .collect())
    }
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError> {
        Ok(
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name = $1)")
                .bind(timezone)
                .fetch_one(&self.pool)
                .await?,
        )
    }

    async fn list(&self) -> Result<Vec<DailyTask>, DaoError> {
        sqlx::query("DELETE FROM daily_tasks d USING tasks t, categories c WHERE d.task_id = t.id AND t.category_id = c.id AND c.user_id = $1 AND d.expires_at <= statement_timestamp()")
            .bind(self.user_id).execute(&self.pool).await?;
        let rows: Vec<(i64, String, i64)> = sqlx::query_as("SELECT d.task_id, d.subtask_id, (extract(epoch FROM d.expires_at) * 1000)::bigint FROM daily_tasks d JOIN tasks t ON t.id = d.task_id JOIN categories c ON c.id = t.category_id WHERE c.user_id = $1 AND d.expires_at > statement_timestamp() AND (d.subtask_id = '' OR EXISTS (SELECT 1 FROM jsonb_array_elements(t.subtasks) s WHERE s->>'id' = d.subtask_id)) ORDER BY d.task_id, d.subtask_id")
            .bind(self.user_id).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(task_id, subtask_id, expires_at)| DailyTask {
                task_id,
                subtask_id,
                expires_at,
            })
            .collect())
    }

    async fn select(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: &str,
    ) -> Result<bool, DaoError> {
        let mut tx = self.pool.begin().await?;
        let row: Option<(bool, Json<Vec<Subtask>>)> = sqlx::query_as("SELECT t.completed, t.subtasks FROM tasks t JOIN categories c ON c.id = t.category_id WHERE t.id = $1 AND c.user_id = $2 FOR UPDATE OF t")
            .bind(task_id).bind(self.user_id).fetch_optional(&mut *tx).await?;
        let Some((completed, Json(mut steps))) = row else {
            return Ok(false);
        };
        if completed {
            return Err(DaoError::TaskArchived);
        }
        let subtask_id = if let Some(index) = index {
            let Some(step) = steps.get_mut(index) else {
                return Ok(false);
            };
            if step.id.is_none() {
                step.id = Some(
                    sqlx::query_scalar::<_, String>("SELECT nextval('subtask_identity')::text")
                        .fetch_one(&mut *tx)
                        .await?,
                );
            }
            let id = step.id.clone().unwrap();
            sqlx::query("UPDATE tasks SET subtasks = $2 WHERE id = $1")
                .bind(task_id)
                .bind(Json(&steps))
                .execute(&mut *tx)
                .await?;
            id
        } else {
            String::new()
        };
        // Calendar-day arithmetic preserves local 02:00 across DST transitions.
        sqlx::query("INSERT INTO daily_tasks(task_id, subtask_id, expires_at, timezone) VALUES ($1, $2, daily_reset_at($3, statement_timestamp()), $3) ON CONFLICT (task_id, subtask_id) DO UPDATE SET expires_at = EXCLUDED.expires_at, timezone = EXCLUDED.timezone")
            .bind(task_id).bind(subtask_id).bind(timezone).execute(&mut *tx).await?;
        sqlx::query(
            "DELETE FROM daily_tasks WHERE task_id = $1 AND expires_at <= statement_timestamp()",
        )
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn remove(&self, task_id: i64, subtask_id: &str) -> Result<(), DaoError> {
        sqlx::query("DELETE FROM daily_tasks d USING tasks t, categories c WHERE d.task_id = $1 AND d.subtask_id = $2 AND t.id = d.task_id AND c.id = t.category_id AND c.user_id = $3")
            .bind(task_id).bind(subtask_id).bind(self.user_id).execute(&self.pool).await?;
        Ok(())
    }
}
