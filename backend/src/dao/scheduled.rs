use super::DaoError;
use crate::domain::{ScheduledOccurrence, ScheduledTask, ScheduledTaskInput, Subtask};
use async_trait::async_trait;
use sqlx::{FromRow, PgPool, types::Json};

#[async_trait]
pub trait ScheduledDao: Send + Sync {
    async fn list(&self) -> Result<Vec<ScheduledTask>, DaoError>;
    async fn save(
        &self,
        id: Option<i64>,
        input: &ScheduledTaskInput,
    ) -> Result<Option<ScheduledTask>, DaoError>;
    async fn delete(&self, id: i64) -> Result<bool, DaoError>;
    async fn daily(&self) -> Result<Vec<ScheduledOccurrence>, DaoError>;
    async fn progress(
        &self,
        id: i64,
        completed: Option<bool>,
        steps: Option<&[Subtask]>,
    ) -> Result<bool, DaoError>;
    async fn dismiss(&self, id: i64) -> Result<bool, DaoError>;
    async fn select_today(&self, id: i64) -> Result<bool, DaoError>;
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError>;
}

pub struct PgScheduledDao {
    pool: PgPool,
    user_id: i64,
}

pub async fn generate_scheduled(pool: &PgPool, owner: Option<i64>) -> Result<(), DaoError> {
    sqlx::query("SELECT generate_scheduled_occurrences($1, statement_timestamp())")
        .bind(owner)
        .execute(pool)
        .await?;
    Ok(())
}

impl PgScheduledDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self { pool, user_id }
    }

    async fn templates(&self, id: Option<i64>) -> Result<Vec<ScheduledTask>, DaoError> {
        sqlx::query_as::<_, TemplateRow>("SELECT s.id, s.description, s.detailed_description, s.weekdays, s.timezone, s.subtasks,
            CASE WHEN o.id IS NOT NULL THEN
                CASE WHEN NOT o.completed AND o.expires_at > statement_timestamp() THEN (extract(epoch FROM o.expires_at)*1000)::bigint END
            WHEN extract(isodow FROM (daily_reset_at(s.timezone, statement_timestamp()) AT TIME ZONE s.timezone)::date - 1)::smallint = ANY(s.weekdays)
                THEN (extract(epoch FROM daily_reset_at(s.timezone, statement_timestamp()))*1000)::bigint END AS available_today_until
            FROM scheduled_tasks s LEFT JOIN LATERAL (
                SELECT o.id, o.completed, o.expires_at FROM scheduled_occurrences o WHERE o.scheduled_task_id=s.id
                    AND o.planning_date = (daily_reset_at(o.timezone, statement_timestamp()) AT TIME ZONE o.timezone)::date - 1
                ORDER BY o.id DESC LIMIT 1
            ) o ON true
            WHERE s.user_id=$1 AND ($2::bigint IS NULL OR s.id=$2) ORDER BY s.id")
            .bind(self.user_id).bind(id).fetch_all(&self.pool).await?.into_iter().map(TemplateRow::entity).collect()
    }
}

#[derive(FromRow)]
struct TemplateRow {
    id: i64,
    description: String,
    detailed_description: String,
    weekdays: Vec<i16>,
    timezone: String,
    subtasks: Json<Vec<Subtask>>,
    available_today_until: Option<i64>,
}
impl TemplateRow {
    fn entity(self) -> Result<ScheduledTask, DaoError> {
        Ok(ScheduledTask {
            id: self.id,
            available_today_until: self.available_today_until,
            details: ScheduledTaskInput {
                description: self.description,
                detailed_description: self.detailed_description,
                weekdays: self.weekdays,
                timezone: self.timezone,
                subtasks: self.subtasks.0,
            },
        })
    }
}

#[derive(FromRow)]
struct OccurrenceRow {
    id: i64,
    scheduled_task_id: i64,
    description: String,
    detailed_description: String,
    subtasks: Json<Vec<Subtask>>,
    completed: bool,
    expires_at: i64,
}

#[async_trait]
impl ScheduledDao for PgScheduledDao {
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError> {
        Ok(
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name = $1)")
                .bind(timezone)
                .fetch_one(&self.pool)
                .await?,
        )
    }
    async fn list(&self) -> Result<Vec<ScheduledTask>, DaoError> {
        self.templates(None).await
    }
    async fn save(
        &self,
        id: Option<i64>,
        input: &ScheduledTaskInput,
    ) -> Result<Option<ScheduledTask>, DaoError> {
        let row = if let Some(id) = id {
            sqlx::query_scalar::<_, i64>("UPDATE scheduled_tasks SET description=$3, weekdays=$4, timezone=$5, subtasks=$6, detailed_description=$7 WHERE id=$1 AND user_id=$2 RETURNING id")
                .bind(id).bind(self.user_id).bind(&input.description)
                .bind(&input.weekdays).bind(&input.timezone).bind(Json(&input.subtasks)).bind(&input.detailed_description).fetch_optional(&self.pool).await?
        } else {
            sqlx::query_scalar::<_, i64>("INSERT INTO scheduled_tasks(user_id, description, weekdays, timezone, subtasks, detailed_description) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
                .bind(self.user_id).bind(&input.description)
                .bind(&input.weekdays).bind(&input.timezone).bind(Json(&input.subtasks)).bind(&input.detailed_description).fetch_optional(&self.pool).await?
        };
        match row {
            Some(id) => Ok(self.templates(Some(id)).await?.pop()),
            None => Ok(None),
        }
    }
    async fn delete(&self, id: i64) -> Result<bool, DaoError> {
        Ok(
            sqlx::query("DELETE FROM scheduled_tasks WHERE id=$1 AND user_id=$2")
                .bind(id)
                .bind(self.user_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }
    async fn daily(&self) -> Result<Vec<ScheduledOccurrence>, DaoError> {
        generate_scheduled(&self.pool, Some(self.user_id)).await?;
        let rows = sqlx::query_as::<_, OccurrenceRow>("SELECT o.id, o.scheduled_task_id, o.description, o.detailed_description, o.subtasks, o.completed, (extract(epoch FROM o.expires_at)*1000)::bigint AS expires_at FROM scheduled_occurrences o JOIN scheduled_tasks s ON s.id=o.scheduled_task_id WHERE s.user_id=$1 AND o.selected AND NOT o.hidden AND o.expires_at > statement_timestamp() ORDER BY o.id")
            .bind(self.user_id).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|r| {
                Ok(ScheduledOccurrence {
                    id: r.id,
                    scheduled_task_id: r.scheduled_task_id,
                    description: r.description,
                    detailed_description: r.detailed_description,
                    subtasks: r.subtasks.0,
                    completed: r.completed,
                    expires_at: r.expires_at,
                })
            })
            .collect()
    }
    async fn progress(
        &self,
        id: i64,
        completed: Option<bool>,
        steps: Option<&[Subtask]>,
    ) -> Result<bool, DaoError> {
        let mut tx = self.pool.begin().await?;
        let current: Option<Json<Vec<Subtask>>> = sqlx::query_scalar("SELECT o.subtasks FROM scheduled_occurrences o JOIN scheduled_tasks s ON s.id=o.scheduled_task_id WHERE o.id=$1 AND s.user_id=$2 AND o.selected AND NOT o.hidden AND o.expires_at > statement_timestamp() FOR UPDATE OF o")
            .bind(id).bind(self.user_id).fetch_optional(&mut *tx).await?;
        let Some(current) = current else {
            return Ok(false);
        };
        let steps = steps.unwrap_or(&current.0);
        let completed = completed.unwrap_or(!steps.is_empty() && steps.iter().all(|s| s.completed));
        sqlx::query("UPDATE scheduled_occurrences SET completed=$2, subtasks=$3 WHERE id=$1")
            .bind(id)
            .bind(completed)
            .bind(Json(steps))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }
    async fn dismiss(&self, id: i64) -> Result<bool, DaoError> {
        Ok(sqlx::query("UPDATE scheduled_occurrences o SET hidden=true FROM scheduled_tasks s WHERE o.id=$1 AND s.id=o.scheduled_task_id AND s.user_id=$2 AND o.selected AND o.expires_at > statement_timestamp()")
            .bind(id).bind(self.user_id).execute(&self.pool).await?.rows_affected() > 0)
    }
    async fn select_today(&self, id: i64) -> Result<bool, DaoError> {
        // Generate lazily so a newly created due schedule can be selected immediately.
        generate_scheduled(&self.pool, Some(self.user_id)).await?;
        Ok(sqlx::query("UPDATE scheduled_occurrences o SET selected=true, hidden=false
            FROM scheduled_tasks s WHERE o.scheduled_task_id=s.id AND s.id=$1 AND s.user_id=$2
                AND NOT o.completed AND o.expires_at > statement_timestamp()
                AND o.planning_date = (daily_reset_at(o.timezone, statement_timestamp()) AT TIME ZONE o.timezone)::date - 1")
            .bind(id).bind(self.user_id).execute(&self.pool).await?.rows_affected() > 0)
    }
}
