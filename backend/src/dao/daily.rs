use super::DaoError;
use crate::domain::{DailyTask, DailyTaskSource, DailyTaskSummary, Subtask, Task};
use async_trait::async_trait;
use sqlx::{PgPool, types::Json};

type DailySummaryRow = (
    i64,
    String,
    String,
    Option<String>,
    bool,
    i64,
    Option<i64>,
    bool,
);

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
    async fn select_date(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: &str,
        date: &str,
    ) -> Result<bool, DaoError>;
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError>;
}

#[async_trait]
pub trait DailyPlanningDao: Send + Sync {
    async fn plan(
        &self,
        listed: &Task,
        index: Option<usize>,
        date: &str,
        timezone: Option<&str>,
    ) -> Result<Option<super::daily_work::DailyMove>, DaoError>;
}

#[async_trait]
impl DailyPlanningDao for PgDailyDao {
    async fn plan(
        &self,
        listed: &Task,
        index: Option<usize>,
        date: &str,
        timezone: Option<&str>,
    ) -> Result<Option<super::daily_work::DailyMove>, DaoError> {
        let dao = if date == "today" {
            Self::new(self.pool.clone(), self.user_id)
        } else {
            Self::tomorrow(self.pool.clone(), self.user_id)
        };
        dao.select_guarded(listed.id, index, timezone, Some(date), Some(listed))
            .await
    }
}

pub struct PgDailyDao {
    pool: PgPool,
    user_id: i64,
    table: &'static str,
}
impl PgDailyDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self {
            pool,
            user_id,
            table: "daily_tasks",
        }
    }

    pub fn tomorrow(pool: PgPool, user_id: i64) -> Self {
        Self {
            pool,
            user_id,
            table: "tomorrow_tasks",
        }
    }
}

#[async_trait]
impl DailyDao for PgDailyDao {
    async fn summary(&self) -> Result<Vec<DailyTaskSummary>, DaoError> {
        super::generate_scheduled(&self.pool, Some(self.user_id)).await?;
        let rows: Vec<DailySummaryRow> = sqlx::query_as(
            "SELECT t.id, c.description, t.description, s.step->>'description',
                t.completed OR COALESCE((s.step->>'completed')::boolean, false), COALESCE(s.position, 0), c.id, false
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
              SELECT -o.id, '', o.description, s.step->>'description',
                   o.completed OR COALESCE((s.step->>'completed')::boolean, false), COALESCE(s.position, 0), NULL::bigint, false
              FROM scheduled_occurrences o JOIN scheduled_tasks st ON st.id=o.scheduled_task_id
              LEFT JOIN LATERAL jsonb_array_elements(o.subtasks) WITH ORDINALITY AS s(step, position) ON true
               WHERE st.user_id=$1 AND o.selected AND NOT o.hidden AND o.expires_at > statement_timestamp()
             UNION ALL
               SELECT r.id, '⏰ Reminders', r.description || ' — ' || to_char(r.due_at AT TIME ZONE r.timezone, CASE WHEN r.all_day THEN 'YYYY-MM-DD' ELSE 'YYYY-MM-DD HH24:MI' END) || CASE WHEN r.all_day THEN ' (All day)' ELSE '' END || ' (' || r.timezone || ')',
                   NULL::text, r.completed, 0::bigint, NULL::bigint, true
               FROM reminder_tasks r WHERE r.user_id=$1 AND r.selected_until > statement_timestamp()
             ORDER BY 1, 6",
        )
        .bind(self.user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(task_id, category, task, subtask, completed, _, category_id, reminder)| {
                    DailyTaskSummary {
                        source: if reminder {
                            DailyTaskSource::Reminder
                        } else if category_id.is_some() {
                            DailyTaskSource::Ordinary
                        } else {
                            DailyTaskSource::Scheduled
                        },
                        task_id,
                        category_id,
                        category,
                        task,
                        subtask,
                        completed,
                    }
                },
            )
            .collect())
    }
    async fn valid_timezone(&self, timezone: &str) -> Result<bool, DaoError> {
        Ok(crate::planning::zone(timezone).is_ok())
    }

    async fn list(&self) -> Result<Vec<DailyTask>, DaoError> {
        let table = self.table;
        let mut tx = self.pool.begin().await?;
        let instant = super::maintenance::now(&mut tx).await?;
        super::maintenance::expire_picks(&mut tx, table, Some(self.user_id), None, instant).await?;
        tx.commit().await?;
        let rows: Vec<(i64, String, i64, String)> = sqlx::query_as(&format!("SELECT d.task_id, d.subtask_id, (extract(epoch FROM d.expires_at) * 1000)::bigint, d.timezone FROM {table} d JOIN tasks t ON t.id = d.task_id JOIN categories c ON c.id = t.category_id WHERE c.user_id = $1 AND d.expires_at > statement_timestamp() AND (d.subtask_id = '' OR EXISTS (SELECT 1 FROM jsonb_array_elements(t.subtasks) s WHERE s->>'id' = d.subtask_id)) ORDER BY d.task_id, d.subtask_id"))
            .bind(self.user_id).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .filter(|(_, _, expiry, timezone)| {
                table != "tomorrow_tasks"
                    || crate::planning::zone(timezone).is_ok_and(|zone| {
                        crate::planning::tomorrow_reset(zone, instant).timestamp_millis() == *expiry
                    })
            })
            .map(|(task_id, subtask_id, expires_at, _)| DailyTask {
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
        self.select_on(task_id, index, timezone, None).await
    }

    async fn select_date(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: &str,
        date: &str,
    ) -> Result<bool, DaoError> {
        self.select_on(task_id, index, timezone, Some(date)).await
    }

    async fn remove(&self, task_id: i64, subtask_id: &str) -> Result<(), DaoError> {
        let table = self.table;
        let mut tx = self.pool.begin().await?;
        let instant = super::maintenance::now(&mut tx).await?;
        super::maintenance::expire_picks(
            &mut tx,
            table,
            Some(self.user_id),
            Some(task_id),
            instant,
        )
        .await?;
        sqlx::query(&format!("DELETE FROM {table} d USING tasks t, categories c WHERE d.task_id = $1 AND ($2 = '' OR d.subtask_id = $2) AND t.id = d.task_id AND c.id = t.category_id AND c.user_id = $3"))
            .bind(task_id).bind(subtask_id).bind(self.user_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

impl PgDailyDao {
    async fn select_on(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: &str,
        date: Option<&str>,
    ) -> Result<bool, DaoError> {
        Ok(self
            .select_guarded(task_id, index, Some(timezone), date, None)
            .await?
            .is_some())
    }

    async fn select_guarded(
        &self,
        task_id: i64,
        index: Option<usize>,
        timezone: Option<&str>,
        date: Option<&str>,
        listed: Option<&Task>,
    ) -> Result<Option<super::daily_work::DailyMove>, DaoError> {
        // Table names are fixed constructor values, never user input.
        let table = self.table;
        let mut tx = self.pool.begin().await?;
        let row: Option<(bool, Json<Vec<Subtask>>, String, i64)> = sqlx::query_as("SELECT t.completed, t.subtasks, t.description, t.category_id FROM tasks t JOIN categories c ON c.id = t.category_id WHERE t.id = $1 AND c.user_id = $2 FOR UPDATE OF t")
            .bind(task_id).bind(self.user_id).fetch_optional(&mut *tx).await?;
        let Some((completed, Json(mut steps), description, category_id)) = row else {
            return Ok(None);
        };
        if listed.is_some_and(|task| {
            task.description != description
                || task.category_id != category_id
                || task.subtasks.len() != steps.len()
                || task
                    .subtasks
                    .iter()
                    .zip(&steps)
                    .any(|(old, new)| old.id != new.id || old.description != new.description)
        }) {
            return Ok(None);
        }
        if completed {
            return Err(DaoError::TaskArchived);
        }
        if index.is_some_and(|index| index >= steps.len()) {
            return Ok(None);
        }
        let saved_timezone: String;
        let timezone = match timezone {
            Some(timezone) => timezone,
            None => {
                saved_timezone = sqlx::query_scalar("SELECT COALESCE(
                    (SELECT p.timezone FROM daily_tasks p JOIN tasks t ON t.id=p.task_id JOIN categories c ON c.id=t.category_id WHERE c.user_id=$1 ORDER BY p.expires_at DESC,t.id DESC LIMIT 1),
                    (SELECT p.timezone FROM tomorrow_tasks p JOIN tasks t ON t.id=p.task_id JOIN categories c ON c.id=t.category_id WHERE c.user_id=$1 ORDER BY p.expires_at DESC,t.id DESC LIMIT 1),
                    (SELECT timezone FROM morning_offers WHERE user_id=$1 ORDER BY reset_at DESC,id DESC LIMIT 1),
                    (SELECT timezone FROM scheduled_tasks WHERE user_id=$1 ORDER BY id DESC LIMIT 1),
                    (SELECT timezone FROM reminder_tasks WHERE user_id=$1 ORDER BY id DESC LIMIT 1),'UTC')")
                    .bind(self.user_id).fetch_one(&mut *tx).await?;
                &saved_timezone
            }
        };
        let zone = crate::planning::zone(timezone)
            .map_err(|_| DaoError::InvalidMove("invalid timezone"))?;
        let instant = super::maintenance::now(&mut tx).await?;
        super::maintenance::expire_picks(
            &mut tx,
            table,
            Some(self.user_id),
            Some(task_id),
            instant,
        )
        .await?;
        let (day, reset) = if table == "tomorrow_tasks" {
            let (day, zone) =
                super::daily_move::target(date.unwrap_or("tomorrow"), timezone, instant)?;
            (day, crate::planning::at(zone, day, 2))
        } else {
            (
                instant.with_timezone(&zone).date_naive(),
                crate::planning::daily_reset(zone, instant),
            )
        };
        let mut subtask_ids = Vec::new();
        for (position, step) in steps.iter_mut().enumerate() {
            if index.is_some_and(|index| index != position) {
                continue;
            }
            if step.id.is_none() {
                step.id = Some(
                    sqlx::query_scalar::<_, String>("SELECT nextval('subtask_identity')::text")
                        .fetch_one(&mut *tx)
                        .await?,
                );
            }
            subtask_ids.push(step.id.clone().unwrap());
        }
        if !subtask_ids.is_empty() {
            sqlx::query("UPDATE tasks SET subtasks = $2 WHERE id = $1")
                .bind(task_id)
                .bind(Json(&steps))
                .execute(&mut *tx)
                .await?;
        } else {
            subtask_ids.push(String::new());
        }
        // A parent with steps selects those steps, never a duplicate parent pick.
        if index.is_none() && !steps.is_empty() {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE task_id = $1 AND subtask_id = ''"
            ))
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        }
        // Calendar-day arithmetic preserves local 02:00 across DST transitions.
        if table == "tomorrow_tasks" {
            for subtask_id in &subtask_ids {
                sqlx::query("DELETE FROM morning_offer_items WHERE task_id=$1 AND subtask_id=$2")
                    .bind(task_id)
                    .bind(subtask_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        sqlx::query(&format!("INSERT INTO {table}(task_id, subtask_id, expires_at, timezone) SELECT $1, id, $4, $3 FROM unnest($2::text[]) AS ids(id) ON CONFLICT (task_id, subtask_id) DO UPDATE SET expires_at = EXCLUDED.expires_at, timezone = EXCLUDED.timezone"))
            .bind(task_id).bind(&subtask_ids).bind(timezone).bind(reset).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(super::daily_work::DailyMove {
            date: day.to_string(),
            timezone: timezone.into(),
        }))
    }
}
