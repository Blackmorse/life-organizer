use async_trait::async_trait;
use sqlx::{PgConnection, PgPool, types::Json};

use crate::domain::{Subtask, Task, TaskUrgency};

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
    async fn set_completed(&self, id: i64, completed: bool) -> Result<Option<Task>, DaoError>;
    async fn replace_subtasks(
        &self,
        id: i64,
        subtasks: &[Subtask],
    ) -> Result<Option<Task>, DaoError>;
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

    /// Serialize edits/deletes against completion so an archived task cannot be
    /// changed by a concurrent request that started before it was completed.
    async fn lock_active_task(
        &self,
        connection: &mut PgConnection,
        id: i64,
    ) -> Result<bool, DaoError> {
        let completed: Option<bool> = sqlx::query_scalar("SELECT t.completed FROM tasks t JOIN categories c ON c.id = t.category_id WHERE t.id = $1 AND c.user_id = $2 FOR UPDATE OF t")
            .bind(id)
            .bind(self.user_id)
            .fetch_optional(connection)
            .await?;
        match completed {
            Some(true) => Err(DaoError::TaskArchived),
            Some(false) => Ok(true),
            None => Ok(false),
        }
    }
}

fn task(
    (id, category_id, description, urgency, subtasks, completed): (
        i64,
        i64,
        String,
        String,
        Json<Vec<Subtask>>,
        bool,
    ),
) -> Result<Task, DaoError> {
    Ok(Task {
        id,
        category_id,
        description,
        subtasks: subtasks.0,
        completed,
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
        let row = sqlx::query_as("INSERT INTO tasks (category_id, description, urgency) SELECT id, $2, $4 FROM categories WHERE id = $1 AND user_id = $3 RETURNING id, category_id, description, urgency, subtasks, completed")
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
        let row = sqlx::query_as("SELECT t.id, t.category_id, t.description, t.urgency, t.subtasks, t.completed FROM tasks t JOIN categories c ON c.id = t.category_id WHERE t.id = $1 AND c.user_id = $2")
            .bind(id)
            .bind(self.user_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(task).transpose()
    }

    async fn list(&self, category_id: Option<i64>) -> Result<Vec<Task>, DaoError> {
        let rows = sqlx::query_as("SELECT t.id, t.category_id, t.description, t.urgency, t.subtasks, t.completed FROM tasks t JOIN categories c ON c.id = t.category_id WHERE ($1::bigint IS NULL OR t.category_id = $1) AND c.user_id = $2 ORDER BY t.id")
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
        let mut transaction = self.pool.begin().await?;
        if !self.lock_active_task(&mut transaction, id).await? {
            return Ok(None);
        }
        let row = sqlx::query_as("UPDATE tasks t SET category_id = $2, description = $3, urgency = $5 WHERE t.id = $1 AND EXISTS (SELECT 1 FROM categories c WHERE c.id = t.category_id AND c.user_id = $4) AND EXISTS (SELECT 1 FROM categories c WHERE c.id = $2 AND c.user_id = $4) RETURNING id, category_id, description, urgency, subtasks, completed")
            .bind(id)
            .bind(category_id)
            .bind(description)
            .bind(self.user_id)
            .bind(urgency.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(write_error)?;
        if row.is_none() {
            return Err(DaoError::CategoryMissing);
        }
        transaction.commit().await?;
        row.map(task).transpose()
    }

    async fn delete(&self, id: i64) -> Result<bool, DaoError> {
        let mut transaction = self.pool.begin().await?;
        if !self.lock_active_task(&mut transaction, id).await? {
            return Ok(false);
        }
        let result = sqlx::query("DELETE FROM tasks t USING categories c WHERE t.id = $1 AND c.id = t.category_id AND c.user_id = $2")
            .bind(id)
            .bind(self.user_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    async fn replace_subtasks(
        &self,
        id: i64,
        subtasks: &[Subtask],
    ) -> Result<Option<Task>, DaoError> {
        let mut transaction = self.pool.begin().await?;
        if !self.lock_active_task(&mut transaction, id).await? {
            return Ok(None);
        }
        let Json(previous): Json<Vec<Subtask>> =
            sqlx::query_scalar("SELECT subtasks FROM tasks WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *transaction)
                .await?;
        let mut seen = std::collections::HashSet::new();
        for step_id in subtasks.iter().filter_map(|step| step.id.as_ref()) {
            if !seen.insert(step_id)
                || !previous
                    .iter()
                    .any(|step| step.id.as_ref() == Some(step_id))
            {
                return Err(DaoError::InvalidSubtaskIds);
            }
        }
        // Empty lists do not complete tasks. Save steps and derived parent state
        // in the same write, under the existing archive/ownership row lock.
        let completed = !subtasks.is_empty() && subtasks.iter().all(|step| step.completed);
        let row = sqlx::query_as("UPDATE tasks t SET subtasks = $3, completed = $4 WHERE t.id = $1 AND EXISTS (SELECT 1 FROM categories c WHERE c.id = t.category_id AND c.user_id = $2) RETURNING id, category_id, description, urgency, subtasks, completed")
            .bind(id)
            .bind(self.user_id)
            .bind(Json(subtasks))
            .bind(completed)
            .fetch_optional(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM daily_tasks d WHERE d.task_id = $1 AND d.subtask_id <> '' AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements($2::jsonb) s WHERE s->>'id' = d.subtask_id)")
            .bind(id).bind(Json(subtasks)).execute(&mut *transaction).await?;
        transaction.commit().await?;
        row.map(task).transpose()
    }

    async fn set_completed(&self, id: i64, completed: bool) -> Result<Option<Task>, DaoError> {
        let row = sqlx::query_as("UPDATE tasks t SET completed = $3 WHERE t.id = $1 AND EXISTS (SELECT 1 FROM categories c WHERE c.id = t.category_id AND c.user_id = $2) RETURNING id, category_id, description, urgency, subtasks, completed")
            .bind(id)
            .bind(self.user_id)
            .bind(completed)
            .fetch_optional(&self.pool)
            .await?;
        row.map(task).transpose()
    }
}
