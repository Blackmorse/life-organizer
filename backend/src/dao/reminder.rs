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
    async fn select_today(&self, id: i64) -> Result<Option<ReminderTask>, DaoError>;
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
    detailed_description: String,
    due_at: i64,
    timezone: String,
    completed: bool,
    expires_at: Option<i64>,
    available_today_until: Option<i64>,
}
impl From<ReminderRow> for ReminderTask {
    fn from(r: ReminderRow) -> Self {
        Self {
            id: r.id,
            details: ReminderTaskInput {
                description: r.description,
                detailed_description: r.detailed_description,
                due_at: r.due_at,
                timezone: r.timezone,
            },
            completed: r.completed,
            expires_at: r.expires_at,
            available_today_until: r.available_today_until,
        }
    }
}
const COLUMNS: &str = "id, description, detailed_description, (extract(epoch FROM due_at)*1000)::bigint AS due_at, timezone, completed, CASE WHEN selected_until > statement_timestamp() THEN (extract(epoch FROM selected_until)*1000)::bigint END AS expires_at,
    CASE WHEN NOT completed AND (due_at AT TIME ZONE timezone)::date = (statement_timestamp() AT TIME ZONE timezone)::date
        THEN (extract(epoch FROM (((due_at AT TIME ZONE timezone)::date + 1 + time '00:00') AT TIME ZONE timezone))*1000)::bigint END AS available_today_until";

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
                due_at=to_timestamp($4::double precision/1000), timezone=$5, detailed_description=$6 WHERE id=$1 AND user_id=$2 RETURNING {COLUMNS}"))
                .bind(id).bind(self.user_id).bind(&input.description).bind(input.due_at).bind(&input.timezone).bind(&input.detailed_description).fetch_optional(&self.pool).await?
        } else {
            sqlx::query_as::<_, ReminderRow>(&format!("INSERT INTO reminder_tasks(user_id, description, due_at, timezone, detailed_description) VALUES ($1,$2,to_timestamp($3::double precision/1000),$4,$5) RETURNING {COLUMNS}"))
                .bind(self.user_id).bind(&input.description).bind(input.due_at).bind(&input.timezone).bind(&input.detailed_description).fetch_optional(&self.pool).await?
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
    async fn select_today(&self, id: i64) -> Result<Option<ReminderTask>, DaoError> {
        let mut tx = self.pool.begin().await?;
        let row: Option<(chrono::DateTime<chrono::Utc>,String)> = sqlx::query_as("SELECT due_at,timezone FROM reminder_tasks WHERE id=$1 AND user_id=$2 AND NOT completed FOR UPDATE")
            .bind(id).bind(self.user_id).fetch_optional(&mut *tx).await?;
        let Some((event, timezone)) = row else {
            return Ok(None);
        };
        let instant = super::maintenance::now(&mut tx).await?;
        let zone = super::maintenance::timezone(&timezone)?;
        let day = event.with_timezone(&zone).date_naive();
        if day != instant.with_timezone(&zone).date_naive() {
            return Ok(None);
        }
        let expiry = crate::planning::at(zone, day.succ_opt().unwrap(), 2);
        let result = sqlx::query_as::<_, ReminderRow>(&format!(
            "UPDATE reminder_tasks SET selected_until=$2 WHERE id=$1 RETURNING {COLUMNS}"
        ))
        .bind(id)
        .bind(expiry)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(result.into()))
    }
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError> {
        Ok(crate::planning::zone(timezone).is_ok())
    }
}
