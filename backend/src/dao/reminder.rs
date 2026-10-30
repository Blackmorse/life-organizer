use super::DaoError;
use crate::domain::{ReminderTask, ReminderTaskInput};
use async_trait::async_trait;
use sqlx::{FromRow, PgPool};

#[async_trait]
pub trait ReminderDao: Send + Sync {
    async fn list(&self) -> Result<Vec<ReminderTask>, DaoError>;
    async fn save(
        &self,
        id: Option<i64>,
        input: &ReminderTaskInput,
    ) -> Result<Option<ReminderTask>, DaoError>;
    async fn delete(&self, id: i64) -> Result<bool, DaoError>;
    async fn complete(&self, id: i64, completed: bool) -> Result<Option<ReminderTask>, DaoError>;
    async fn dismiss(&self, id: i64) -> Result<bool, DaoError>;
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError>;
}

pub struct PgReminderDao {
    pool: PgPool,
    user_id: i64,
}
impl PgReminderDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self { pool, user_id }
    }
}

#[derive(FromRow)]
struct ReminderRow {
    id: i64,
    description: String,
    due_at: i64,
    timezone: String,
    completed: bool,
    expires_at: Option<i64>,
}
impl From<ReminderRow> for ReminderTask {
    fn from(r: ReminderRow) -> Self {
        Self {
            id: r.id,
            details: ReminderTaskInput {
                description: r.description,
                due_at: r.due_at,
                timezone: r.timezone,
            },
            completed: r.completed,
            expires_at: r.expires_at,
        }
    }
}
const COLUMNS: &str = "id, description, (extract(epoch FROM due_at)*1000)::bigint AS due_at, timezone, completed, (extract(epoch FROM selected_until)*1000)::bigint AS expires_at";

#[async_trait]
impl ReminderDao for PgReminderDao {
    async fn list(&self) -> Result<Vec<ReminderTask>, DaoError> {
        Ok(sqlx::query_as::<_, ReminderRow>(&format!(
            "SELECT {COLUMNS} FROM reminder_tasks WHERE user_id=$1 ORDER BY due_at, id"
        ))
        .bind(self.user_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
    }
    async fn save(
        &self,
        id: Option<i64>,
        input: &ReminderTaskInput,
    ) -> Result<Option<ReminderTask>, DaoError> {
        let row = if let Some(id) = id {
            sqlx::query_as::<_, ReminderRow>(&format!("UPDATE reminder_tasks SET description=$3,
                selected_until=CASE WHEN due_at <> to_timestamp($4::double precision/1000) OR timezone <> $5 THEN NULL ELSE selected_until END,
                due_at=to_timestamp($4::double precision/1000), timezone=$5 WHERE id=$1 AND user_id=$2 RETURNING {COLUMNS}"))
                .bind(id).bind(self.user_id).bind(&input.description).bind(input.due_at).bind(&input.timezone).fetch_optional(&self.pool).await?
        } else {
            sqlx::query_as::<_, ReminderRow>(&format!("INSERT INTO reminder_tasks(user_id, description, due_at, timezone) VALUES ($1,$2,to_timestamp($3::double precision/1000),$4) RETURNING {COLUMNS}"))
                .bind(self.user_id).bind(&input.description).bind(input.due_at).bind(&input.timezone).fetch_optional(&self.pool).await?
        };
        Ok(row.map(Into::into))
    }
    async fn delete(&self, id: i64) -> Result<bool, DaoError> {
        Ok(
            sqlx::query("DELETE FROM reminder_tasks WHERE id=$1 AND user_id=$2")
                .bind(id)
                .bind(self.user_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }
    async fn complete(&self, id: i64, completed: bool) -> Result<Option<ReminderTask>, DaoError> {
        Ok(sqlx::query_as::<_, ReminderRow>(&format!(
            "UPDATE reminder_tasks SET completed=$3 WHERE id=$1 AND user_id=$2 RETURNING {COLUMNS}"
        ))
        .bind(id)
        .bind(self.user_id)
        .bind(completed)
        .fetch_optional(&self.pool)
        .await?
        .map(Into::into))
    }
    async fn dismiss(&self, id: i64) -> Result<bool, DaoError> {
        Ok(sqlx::query("UPDATE reminder_tasks SET selected_until=NULL WHERE id=$1 AND user_id=$2 AND selected_until > statement_timestamp()")
            .bind(id).bind(self.user_id).execute(&self.pool).await?.rows_affected() > 0)
    }
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError> {
        Ok(
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name=$1)")
                .bind(timezone)
                .fetch_one(&self.pool)
                .await?,
        )
    }
}
