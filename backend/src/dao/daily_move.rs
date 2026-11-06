//! Atomic removal from today and future planning, shared by Mini App and assistant.
use super::{
    DaoError,
    daily_work::{DailyMove, DailyWork, PgDailyWorkDao},
    maintenance,
};
use crate::{domain::Subtask, planning};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use sqlx::types::Json;

fn target(
    date: &str,
    timezone: &str,
    now: DateTime<Utc>,
) -> Result<(NaiveDate, chrono_tz::Tz), DaoError> {
    let zone = planning::zone(timezone).map_err(|_| DaoError::InvalidMove("invalid timezone"))?;
    let today = now.with_timezone(&zone).date_naive();
    let day = if date == "tomorrow" {
        today.succ_opt()
    } else {
        NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .ok()
            .filter(|day| day.to_string() == date)
    }
    .ok_or(DaoError::InvalidMove("date must be tomorrow or YYYY-MM-DD"))?;
    if day <= today || day.year() > 9998 || day.year() < 1970 {
        return Err(DaoError::InvalidMove(
            "choose a future calendar date (through 9998)",
        ));
    }
    Ok((day, zone))
}

impl PgDailyWorkDao {
    pub(super) async fn move_daily(
        &self,
        listed: &DailyWork,
        date: &str,
        timezone: Option<&str>,
    ) -> Result<Option<DailyMove>, DaoError> {
        let parts: Vec<_> = listed.id.split(':').collect();
        let Some(id) = parts
            .get(1)
            .and_then(|id| id.parse::<i64>().ok())
            .filter(|id| *id > 0)
        else {
            return Ok(None);
        };
        let mut tx = self.pool.begin().await?;
        let result = match parts.as_slice() {
            ["task", _] | ["subtask", _, _] => {
                let row: Option<(String, String, bool, Json<Vec<Subtask>>)> = sqlx::query_as("SELECT t.description,c.description,t.completed,t.subtasks FROM tasks t JOIN categories c ON c.id=t.category_id WHERE t.id=$1 AND c.user_id=$2 FOR UPDATE OF t")
                    .bind(id).bind(self.user_id).fetch_optional(&mut *tx).await?;
                let Some((name, category, completed, Json(steps))) = row else {
                    return Ok(None);
                };
                if completed || name != listed.task || category != listed.category {
                    return Ok(None);
                }
                let step_id = if parts[0] == "subtask" {
                    Some(parts[2])
                } else {
                    None
                };
                if let Some(step_id) = step_id
                    && !steps.iter().any(|s| {
                        s.id.as_deref() == Some(step_id)
                            && !s.completed
                            && Some(&s.description) == listed.subtask.as_ref()
                    })
                {
                    return Ok(None);
                }
                let now = maintenance::now(&mut tx).await?;
                let picks: Vec<(String, String)> = sqlx::query_as("SELECT subtask_id,timezone FROM daily_tasks WHERE task_id=$1 AND ($2::text IS NULL OR subtask_id=$2) AND expires_at > $3 ORDER BY subtask_id FOR UPDATE")
                    .bind(id).bind(step_id).bind(now).fetch_all(&mut *tx).await?;
                let picks: Vec<_> = picks
                    .into_iter()
                    .filter(|(pick, _)| {
                        pick.is_empty() || steps.iter().any(|s| s.id.as_ref() == Some(pick))
                    })
                    .collect();
                let Some((_, saved_zone)) = picks.first() else {
                    return Ok(None);
                };
                if !picks.iter().any(|(pick, _)| {
                    pick.is_empty()
                        || steps
                            .iter()
                            .any(|s| s.id.as_ref() == Some(pick) && !s.completed)
                }) {
                    return Err(DaoError::InvalidMove("completed work cannot be moved"));
                }
                let timezone = timezone.unwrap_or(saved_zone);
                let (day, zone) = target(date, timezone, now)?;
                // Preserve already-expired plans before replacing any future row.
                maintenance::expire_picks(
                    &mut tx,
                    "tomorrow_tasks",
                    Some(self.user_id),
                    Some(id),
                    now,
                )
                .await?;
                for (pick, _) in &picks {
                    let unfinished = pick.is_empty()
                        || steps
                            .iter()
                            .any(|s| s.id.as_ref() == Some(pick) && !s.completed);
                    if !unfinished {
                        continue;
                    }
                    sqlx::query("INSERT INTO tomorrow_tasks(task_id,subtask_id,expires_at,timezone) VALUES($1,$2,$3,$4) ON CONFLICT(task_id,subtask_id) DO UPDATE SET expires_at=EXCLUDED.expires_at,timezone=EXCLUDED.timezone")
                        .bind(id).bind(pick).bind(planning::at(zone, day, 2)).bind(timezone).execute(&mut *tx).await?;
                    // Previously offered carryover must not restore deferred work early.
                    sqlx::query(
                        "DELETE FROM morning_offer_items WHERE task_id=$1 AND subtask_id=$2",
                    )
                    .bind(id)
                    .bind(pick)
                    .execute(&mut *tx)
                    .await?;
                }
                sqlx::query("DELETE FROM daily_tasks WHERE task_id=$1 AND ($2::text IS NULL OR subtask_id=$2) AND expires_at > $3")
                    .bind(id).bind(step_id).bind(now).execute(&mut *tx).await?;
                DailyMove {
                    date: day.to_string(),
                    timezone: timezone.into(),
                }
            }
            ["reminder", _] => {
                let row: Option<(DateTime<Utc>, String)> = sqlx::query_as("SELECT due_at,timezone FROM reminder_tasks WHERE id=$1 AND user_id=$2 AND NOT completed AND selected_until > clock_timestamp() AND description || ' — ' || to_char(due_at AT TIME ZONE timezone, 'YYYY-MM-DD HH24:MI') || ' (' || timezone || ')'=$3 FOR UPDATE")
                    .bind(id).bind(self.user_id).bind(&listed.task).fetch_optional(&mut *tx).await?;
                let Some((due, timezone)) = row else {
                    return Ok(None);
                };
                let now = maintenance::now(&mut tx).await?;
                let (day, zone) = target(date, &timezone, now)?;
                let due =
                    planning::local_instant(zone, day.and_time(due.with_timezone(&zone).time()));
                sqlx::query("UPDATE reminder_tasks SET due_at=$2,selected_until=NULL WHERE id=$1")
                    .bind(id)
                    .bind(due)
                    .execute(&mut *tx)
                    .await?;
                DailyMove {
                    date: day.to_string(),
                    timezone,
                }
            }
            _ => return Err(DaoError::InvalidMove("scheduled tasks cannot be moved")),
        };
        tx.commit().await?;
        Ok(Some(result))
    }
}
