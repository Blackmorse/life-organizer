//! Actionable daily identities. Completion locks the original row and rechecks
//! live selection; the model never chooses an owner or writes task JSON.
use super::DaoError;
use crate::domain::Subtask;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, types::Json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyWork {
    pub id: String,
    pub task: String,
    pub category: String,
    pub subtask: Option<String>,
    pub completed: bool,
}

#[async_trait]
pub trait DailyWorkDao: Send + Sync {
    async fn list(&self) -> Result<Vec<DailyWork>, DaoError>;
    /// False means the listed item changed, expired, or is no longer selected.
    async fn complete(&self, listed: &DailyWork) -> Result<bool, DaoError>;
}

pub struct PgDailyWorkDao {
    pool: PgPool,
    user_id: i64,
}

impl PgDailyWorkDao {
    pub fn new(pool: PgPool, user_id: i64) -> Self {
        Self { pool, user_id }
    }
}

#[async_trait]
impl DailyWorkDao for PgDailyWorkDao {
    async fn list(&self) -> Result<Vec<DailyWork>, DaoError> {
        // Context parents are actionable too: completing one archives the task,
        // exactly like the Mini App's parent completion control.
        let rows: Vec<(String, String, String, Option<String>, bool)> = sqlx::query_as(
            "SELECT 'task:' || t.id, t.description, c.description, NULL::text, t.completed
             FROM tasks t JOIN categories c ON c.id=t.category_id
             WHERE c.user_id=$1 AND EXISTS (SELECT 1 FROM daily_tasks d WHERE d.task_id=t.id
               AND d.expires_at > statement_timestamp() AND (d.subtask_id='' OR EXISTS
               (SELECT 1 FROM jsonb_array_elements(t.subtasks) s WHERE s->>'id'=d.subtask_id)))
             UNION ALL
             SELECT 'subtask:' || t.id || ':' || d.subtask_id, t.description, c.description,
               s.step->>'description', t.completed OR (s.step->>'completed')::boolean
             FROM daily_tasks d JOIN tasks t ON t.id=d.task_id JOIN categories c ON c.id=t.category_id
             JOIN LATERAL jsonb_array_elements(t.subtasks) s(step) ON s.step->>'id'=d.subtask_id
             WHERE c.user_id=$1 AND d.expires_at > statement_timestamp()
             UNION ALL
             SELECT 'scheduled:' || o.id, o.description, 'Scheduled', NULL::text, o.completed
             FROM scheduled_occurrences o JOIN scheduled_tasks s ON s.id=o.scheduled_task_id
             WHERE s.user_id=$1 AND o.selected AND NOT o.hidden AND o.expires_at > statement_timestamp()
             UNION ALL
             SELECT 'scheduled_step:' || o.id || ':' || (s.position-1), o.description, 'Scheduled',
               s.step->>'description', o.completed OR (s.step->>'completed')::boolean
             FROM scheduled_occurrences o JOIN scheduled_tasks st ON st.id=o.scheduled_task_id
             CROSS JOIN LATERAL jsonb_array_elements(o.subtasks) WITH ORDINALITY s(step, position)
             WHERE st.user_id=$1 AND o.selected AND NOT o.hidden AND o.expires_at > statement_timestamp()
             UNION ALL
             SELECT 'reminder:' || r.id, r.description || ' — ' ||
               to_char(r.due_at AT TIME ZONE r.timezone, 'YYYY-MM-DD HH24:MI') || ' (' || r.timezone || ')',
               'Reminders', NULL::text, r.completed
             FROM reminder_tasks r WHERE r.user_id=$1 AND r.selected_until > statement_timestamp()
             ORDER BY 1",
        ).bind(self.user_id).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(id, task, category, subtask, completed)| DailyWork {
                id,
                task,
                category,
                subtask,
                completed,
            })
            .collect())
    }

    async fn complete(&self, listed: &DailyWork) -> Result<bool, DaoError> {
        let parts: Vec<_> = listed.id.split(':').collect();
        let Some(id) = parts
            .get(1)
            .and_then(|id| id.parse::<i64>().ok())
            .filter(|id| *id > 0)
        else {
            return Ok(false);
        };
        let mut tx = self.pool.begin().await?;
        match parts.as_slice() {
            ["task", _] | ["subtask", _, _] => {
                let row: Option<(String, String, bool, Json<Vec<Subtask>>)> = sqlx::query_as(
                    "SELECT t.description, c.description, t.completed, t.subtasks FROM tasks t
                     JOIN categories c ON c.id=t.category_id WHERE t.id=$1 AND c.user_id=$2 FOR UPDATE OF t",
                ).bind(id).bind(self.user_id).fetch_optional(&mut *tx).await?;
                let Some((description, category, completed, Json(mut steps))) = row else {
                    return Ok(false);
                };
                if description != listed.task || category != listed.category {
                    return Ok(false);
                }
                let step_id = if parts[0] == "subtask" {
                    Some(parts[2])
                } else {
                    None
                };
                // Lock picks against removal and check expiry after the task lock.
                let picks: Vec<String> = sqlx::query_scalar(
                    "SELECT subtask_id FROM daily_tasks WHERE task_id=$1 AND expires_at > clock_timestamp()
                     AND ($2::text IS NULL OR subtask_id=$2) FOR UPDATE",
                ).bind(id).bind(step_id).fetch_all(&mut *tx).await?;
                if !picks.iter().any(|pick| {
                    pick.is_empty() || steps.iter().any(|s| s.id.as_ref() == Some(pick))
                }) {
                    return Ok(false);
                }
                if let Some(step_id) = step_id {
                    let Some(step) = steps.iter_mut().find(|s| s.id.as_deref() == Some(step_id))
                    else {
                        return Ok(false);
                    };
                    if Some(&step.description) != listed.subtask.as_ref() {
                        return Ok(false);
                    }
                    if !completed {
                        step.completed = true;
                        let all_done = steps.iter().all(|s| s.completed);
                        let changed = sqlx::query("UPDATE tasks SET subtasks=$2, completed=$3 WHERE id=$1 AND EXISTS (SELECT 1 FROM daily_tasks d WHERE d.task_id=$1 AND d.subtask_id=$4 AND d.expires_at > clock_timestamp())")
                            .bind(id)
                            .bind(Json(&steps))
                            .bind(all_done)
                            .bind(step_id)
                            .execute(&mut *tx)
                            .await?;
                        if changed.rows_affected() == 0 {
                            return Ok(false);
                        }
                    }
                } else {
                    let changed = sqlx::query("UPDATE tasks SET completed=true WHERE id=$1 AND EXISTS (SELECT 1 FROM daily_tasks d WHERE d.task_id=$1 AND d.expires_at > clock_timestamp() AND (d.subtask_id='' OR EXISTS (SELECT 1 FROM jsonb_array_elements(tasks.subtasks) s WHERE s->>'id'=d.subtask_id)))")
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                    if changed.rows_affected() == 0 {
                        return Ok(false);
                    }
                }
            }
            ["scheduled", _] | ["scheduled_step", _, _] => {
                let row: Option<(String, bool, Json<Vec<Subtask>>)> = sqlx::query_as(
                    "SELECT o.description, o.completed, o.subtasks FROM scheduled_occurrences o
                     JOIN scheduled_tasks s ON s.id=o.scheduled_task_id WHERE o.id=$1 AND s.user_id=$2
                     AND o.selected AND NOT o.hidden AND o.expires_at > clock_timestamp() FOR UPDATE OF o",
                ).bind(id).bind(self.user_id).fetch_optional(&mut *tx).await?;
                let Some((description, completed, Json(mut steps))) = row else {
                    return Ok(false);
                };
                if description != listed.task {
                    return Ok(false);
                }
                let done = if parts[0] == "scheduled_step" {
                    let Some(step) = parts[2]
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| steps.get_mut(i))
                    else {
                        return Ok(false);
                    };
                    if Some(&step.description) != listed.subtask.as_ref() {
                        return Ok(false);
                    }
                    step.completed = true;
                    completed || steps.iter().all(|s| s.completed)
                } else {
                    true
                };
                let changed = sqlx::query("UPDATE scheduled_occurrences SET completed=$2, subtasks=$3 WHERE id=$1 AND selected AND NOT hidden AND expires_at > clock_timestamp()")
                    .bind(id).bind(done).bind(Json(&steps)).execute(&mut *tx).await?;
                if changed.rows_affected() == 0 {
                    return Ok(false);
                }
            }
            ["reminder", _] => {
                let changed = sqlx::query(
                    "UPDATE reminder_tasks SET completed=true WHERE id=$1 AND user_id=$2
                     AND selected_until > clock_timestamp() AND description || ' — ' ||
                     to_char(due_at AT TIME ZONE timezone, 'YYYY-MM-DD HH24:MI') || ' (' || timezone || ')'=$3",
                ).bind(id).bind(self.user_id).bind(&listed.task).execute(&mut *tx).await?;
                if changed.rows_affected() == 0 {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
        tx.commit().await?;
        Ok(true)
    }
}
