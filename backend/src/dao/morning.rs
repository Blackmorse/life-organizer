use crate::domain::{DailyTaskSource, DailyTaskSummary};
use sqlx::PgPool;

type MorningSummaryRow = (i64, String, String, Option<String>, i64, Option<i64>);

pub struct MorningStore {
    pool: PgPool,
}

pub struct MorningOffer {
    pub id: i64,
    pub recipient: i64,
    pub items: Vec<DailyTaskSummary>,
    pub tomorrow: Vec<DailyTaskSummary>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Accepted(u64),
    Declined,
    Unavailable,
}

impl MorningStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn sweep(&self) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM daily_tasks WHERE expires_at <= statement_timestamp()")
            .execute(&self.pool)
            .await?;
        sqlx::query("SELECT prepare_scheduled_morning_offers(statement_timestamp())")
            .execute(&self.pool)
            .await?;
        sqlx::query("SELECT prepare_reminder_morning_offers(statement_timestamp())")
            .execute(&self.pool)
            .await?;
        sqlx::query("UPDATE morning_offers SET state = 'expired' WHERE state IN ('pending', 'sent') AND valid_until <= statement_timestamp()")
            .execute(&self.pool).await?;
        Ok(())
    }

    /// A five-minute lease prevents every timer tick from redelivering a failed
    /// or interrupted request. Sent offers are never claimed again.
    pub async fn claim(&self) -> Result<Option<MorningOffer>, sqlx::Error> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "UPDATE morning_offers o SET next_attempt_at = statement_timestamp() + interval '5 minutes'
             FROM users u WHERE o.id = (SELECT id FROM morning_offers
                 WHERE state = 'pending' AND due_at <= statement_timestamp()
                 AND valid_until > statement_timestamp() AND next_attempt_at <= statement_timestamp()
                 ORDER BY due_at, id FOR UPDATE SKIP LOCKED LIMIT 1)
             AND u.id = o.user_id RETURNING o.id, u.telegram_id")
            .fetch_optional(&self.pool).await?;
        let Some((id, recipient)) = row else {
            return Ok(None);
        };
        let rows: Vec<MorningSummaryRow> = sqlx::query_as(
            "SELECT t.id, c.description, t.description, s.step->>'description', COALESCE(s.position, 0), c.id
             FROM morning_offer_items i JOIN morning_offers o ON o.id = i.offer_id
             JOIN tasks t ON t.id = i.task_id JOIN categories c ON c.id = t.category_id
             LEFT JOIN LATERAL (SELECT step, position FROM jsonb_array_elements(t.subtasks)
                 WITH ORDINALITY AS steps(step, position) WHERE step->>'id' = i.subtask_id) s ON true
             WHERE i.offer_id = $1 AND c.user_id = o.user_id AND NOT t.completed
                 AND (i.subtask_id = '' OR (s.step IS NOT NULL AND NOT (s.step->>'completed')::boolean))
              UNION ALL
              SELECT -o.id, '', o.description, step.value->>'description', COALESCE(step.position, 0), NULL::bigint
              FROM morning_offer_scheduled_items i JOIN morning_offers m ON m.id = i.offer_id
              JOIN scheduled_occurrences o ON o.id = i.occurrence_id
              JOIN scheduled_tasks s ON s.id = o.scheduled_task_id
              LEFT JOIN LATERAL jsonb_array_elements(o.subtasks) WITH ORDINALITY AS step(value, position) ON true
              WHERE i.offer_id = $1 AND s.user_id = m.user_id
                  AND NOT o.selected AND NOT o.hidden AND NOT o.completed
                  AND o.expires_at > statement_timestamp()
              ORDER BY 1, 5")
            .bind(id).fetch_all(&self.pool).await?;
        let mut items: Vec<_> = rows
            .into_iter()
            .map(
                |(task_id, category, task, subtask, _, category_id)| DailyTaskSummary {
                    source: if category_id.is_some() {
                        DailyTaskSource::Ordinary
                    } else {
                        DailyTaskSource::Scheduled
                    },
                    task_id,
                    category_id,
                    category,
                    task,
                    subtask,
                    completed: false,
                },
            )
            .collect();
        let reminders: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT r.id, r.description || ' — ' || to_char(r.due_at AT TIME ZONE r.timezone, 'YYYY-MM-DD HH24:MI') || ' (' || r.timezone || ')', i.kind
             FROM morning_offer_reminder_items i JOIN morning_offers m ON m.id=i.offer_id
             JOIN reminder_tasks r ON r.id=i.reminder_id
             WHERE i.offer_id=$1 AND r.user_id=m.user_id AND NOT r.completed
                 AND r.due_at=i.due_at AND r.timezone=i.timezone AND r.selected_until IS NULL
                 AND (i.kind='today' OR (r.due_at AT TIME ZONE r.timezone)::date = (statement_timestamp() AT TIME ZONE r.timezone)::date + 1)
             ORDER BY r.due_at, r.id")
            .bind(id).fetch_all(&self.pool).await?;
        let mut tomorrow = Vec::new();
        for (task_id, task, kind) in reminders {
            let item = DailyTaskSummary {
                source: DailyTaskSource::Reminder,
                task_id,
                category_id: None,
                category: "⏰ Reminders".into(),
                task,
                subtask: None,
                completed: false,
            };
            if kind == "today" {
                items.push(item);
            } else {
                tomorrow.push(item);
            }
        }
        Ok(Some(MorningOffer {
            id,
            recipient,
            items,
            tomorrow,
        }))
    }

    pub async fn sent(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE morning_offers SET state = 'sent' WHERE id = $1 AND state = 'pending'")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn decide(
        &self,
        id: i64,
        telegram_id: i64,
        yes: bool,
    ) -> Result<Decision, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let offer: Option<(i64, String)> = sqlx::query_as(
            "SELECT o.user_id, o.timezone FROM morning_offers o JOIN users u ON u.id = o.user_id
             WHERE o.id = $1 AND u.telegram_id = $2 AND o.state IN ('pending', 'sent')
             AND o.due_at <= statement_timestamp() AND o.valid_until > statement_timestamp() FOR NO KEY UPDATE OF o")
            .bind(id).bind(telegram_id).fetch_optional(&mut *tx).await?;
        let Some((user_id, timezone)) = offer else {
            return Ok(Decision::Unavailable);
        };
        let mut count = 0;
        if yes {
            // Same task-row locks as app edits: completion/deletion cannot race
            // eligibility checking and restoration. Stable order avoids deadlocks.
            sqlx::query("SELECT t.id FROM tasks t JOIN categories c ON c.id = t.category_id WHERE c.user_id = $2 AND EXISTS (SELECT 1 FROM morning_offer_items i WHERE i.offer_id = $1 AND i.task_id = t.id) ORDER BY t.id FOR UPDATE OF t")
                .bind(id).bind(user_id).fetch_all(&mut *tx).await?;
            count = sqlx::query(
                "INSERT INTO daily_tasks(task_id, subtask_id, expires_at, timezone)
                 SELECT t.id, i.subtask_id, daily_reset_at($3, statement_timestamp()), $3
                 FROM morning_offer_items i JOIN tasks t ON t.id = i.task_id JOIN categories c ON c.id = t.category_id
                 WHERE i.offer_id = $1 AND c.user_id = $2 AND NOT t.completed
                   AND (i.subtask_id = '' OR EXISTS (SELECT 1 FROM jsonb_array_elements(t.subtasks) s
                       WHERE s->>'id' = i.subtask_id AND NOT (s->>'completed')::boolean))
                 ORDER BY t.id, i.subtask_id
                 ON CONFLICT(task_id, subtask_id) DO UPDATE SET expires_at = EXCLUDED.expires_at, timezone = EXCLUDED.timezone
                 WHERE daily_tasks.expires_at <= statement_timestamp()")
                .bind(id).bind(user_id).bind(timezone).execute(&mut *tx).await?.rows_affected();
            count += sqlx::query(
                "UPDATE scheduled_occurrences o SET selected = true
                 FROM morning_offer_scheduled_items i, scheduled_tasks s
                 WHERE i.offer_id = $1 AND i.occurrence_id = o.id
                     AND s.id = o.scheduled_task_id AND s.user_id = $2
                     AND NOT o.selected AND NOT o.hidden AND NOT o.completed
                     AND o.expires_at > statement_timestamp()",
            )
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            count += sqlx::query(
                "UPDATE reminder_tasks r SET selected_until=m.valid_until
                 FROM morning_offer_reminder_items i, morning_offers m
                 WHERE i.offer_id=$1 AND m.id=i.offer_id AND i.kind='today'
                     AND r.id=i.reminder_id AND r.user_id=$2 AND NOT r.completed
                     AND r.due_at=i.due_at AND r.timezone=i.timezone AND r.selected_until IS NULL",
            )
            .bind(id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        sqlx::query("UPDATE morning_offers SET state = $2 WHERE id = $1")
            .bind(id)
            .bind(if yes { "accepted" } else { "declined" })
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(if yes {
            Decision::Accepted(count)
        } else {
            Decision::Declined
        })
    }

    pub async fn offset(&self) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT next_update_id FROM bot_poll_state")
            .fetch_one(&self.pool)
            .await
    }

    pub async fn advance(&self, offset: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE bot_poll_state SET next_update_id = GREATEST(next_update_id, $1)")
            .bind(offset)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
