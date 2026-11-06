use crate::domain::{DailyTaskSource, DailyTaskSummary};
use sqlx::PgPool;

type MorningSummaryRow = (i64, String, String, Option<String>, i64, Option<i64>);

pub struct MorningStore {
    pool: PgPool,
    bot_id: i64,
}

pub struct MorningOffer {
    pub id: i64,
    pub recipient: i64,
    pub items: Vec<DailyTaskSummary>,
    pub tomorrow: Vec<DailyTaskSummary>,
    pub yesterday: Vec<DailyTaskSummary>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Accepted(u64),
    Declined,
    Unavailable,
}

impl MorningStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, bot_id: 0 }
    }

    pub fn for_bot(mut self, bot_id: i64) -> Self {
        self.bot_id = bot_id;
        self
    }

    pub async fn sweep(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE reminder_tasks SET selected_until=NULL WHERE selected_until <= statement_timestamp()")
            .execute(&self.pool).await?;
        let mut tx = self.pool.begin().await?;
        let instant = super::maintenance::now(&mut tx).await?;
        // Both tables share parent locks. Acquire their union in stable order
        // before preserving either table, avoiding cross-table lock inversions.
        sqlx::query("SELECT t.id FROM tasks t WHERE EXISTS(SELECT 1 FROM daily_tasks d WHERE d.task_id=t.id AND d.expires_at <= $1) OR EXISTS(SELECT 1 FROM tomorrow_tasks d WHERE d.task_id=t.id AND d.expires_at <= $1) ORDER BY t.id FOR UPDATE")
            .bind(instant).fetch_all(&mut *tx).await?;
        super::maintenance::expire_picks(&mut tx, "tomorrow_tasks", None, None, instant).await?;
        super::maintenance::expire_picks(&mut tx, "daily_tasks", None, None, instant).await?;
        tx.commit().await?;
        super::maintenance::prepare_scheduled_at(&self.pool, instant).await?;
        super::maintenance::prepare_reminders_at(&self.pool, instant).await?;
        super::maintenance::prepare_backlog_at(&self.pool, instant).await?;
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
        self.load(id, recipient).await.map(Some)
    }

    /// Resending keeps the original IDs and decisions; it never reopens an
    /// answered offer or changes its expiry or automatic delivery lease.
    pub async fn unanswered(&self, telegram_id: i64) -> Result<Vec<MorningOffer>, sqlx::Error> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT o.id FROM morning_offers o JOIN users u ON u.id=o.user_id
             WHERE u.telegram_id=$1 AND o.state IN ('pending', 'sent') AND NOT o.backlog_ready
                 AND o.due_at <= statement_timestamp() AND o.valid_until > statement_timestamp()
             ORDER BY o.due_at, o.id",
        )
        .bind(telegram_id)
        .fetch_all(&self.pool)
        .await?;
        let mut offers = Vec::new();
        for id in ids {
            offers.push(self.load(id, telegram_id).await?);
        }
        Ok(offers)
    }

    async fn load(&self, id: i64, recipient: i64) -> Result<MorningOffer, sqlx::Error> {
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
                  AND r.due_at=i.due_at AND r.timezone=i.timezone
                  AND (i.kind='yesterday' OR r.selected_until IS NULL)
                  AND (i.kind='today'
                      OR (i.kind='tomorrow' AND (r.due_at AT TIME ZONE r.timezone)::date = (statement_timestamp() AT TIME ZONE r.timezone)::date + 1)
                      OR (i.kind='yesterday' AND (r.due_at AT TIME ZONE r.timezone)::date = (statement_timestamp() AT TIME ZONE r.timezone)::date - 1))
             ORDER BY r.due_at, r.id")
            .bind(id).fetch_all(&self.pool).await?;
        let mut tomorrow = Vec::new();
        let mut yesterday = Vec::new();
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
            } else if kind == "tomorrow" {
                tomorrow.push(item);
            } else {
                yesterday.push(item);
            }
        }
        Ok(MorningOffer {
            id,
            recipient,
            items,
            tomorrow,
            yesterday,
        })
    }

    pub async fn sent(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE morning_offers SET state = 'sent' WHERE id = $1 AND state = 'pending'")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// No buttons were sent: setup is complete without requiring a decision.
    pub async fn ready_without_choice(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE morning_offers SET state='sent', backlog_ready=true WHERE id=$1 AND state IN ('pending', 'sent')")
            .bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn claim_backlog(&self) -> Result<Option<(i64, i64, i64)>, sqlx::Error> {
        sqlx::query_as(
            "UPDATE morning_offers o SET backlog_next_attempt_at=statement_timestamp() + interval '5 minutes'
             FROM users u WHERE o.id=(SELECT candidate.id FROM morning_offers candidate
                 WHERE candidate.backlog_ready AND NOT candidate.backlog_sent
                     AND candidate.due_at <= statement_timestamp() AND candidate.valid_until > statement_timestamp()
                     AND candidate.backlog_next_attempt_at <= statement_timestamp()
                     AND NOT EXISTS (SELECT 1 FROM morning_offers leased
                         WHERE leased.user_id=candidate.user_id AND leased.backlog_ready
                             AND NOT leased.backlog_sent AND leased.valid_until > statement_timestamp()
                             AND leased.backlog_next_attempt_at > statement_timestamp())
                     AND NOT EXISTS (SELECT 1 FROM morning_offers waiting
                         WHERE waiting.user_id=candidate.user_id AND waiting.state IN ('pending','sent')
                             AND NOT waiting.backlog_ready AND waiting.due_at <= statement_timestamp()
                             AND waiting.valid_until > statement_timestamp())
                     AND NOT EXISTS (SELECT 1 FROM morning_offers delivered
                         WHERE delivered.user_id=candidate.user_id AND delivered.backlog_sent
                             AND (delivered.due_at AT TIME ZONE delivered.timezone)::date
                                 = (candidate.due_at AT TIME ZONE candidate.timezone)::date)
                 ORDER BY candidate.due_at, candidate.id FOR UPDATE SKIP LOCKED LIMIT 1)
                 AND u.id=o.user_id RETURNING o.id, u.id, u.telegram_id",
        ).fetch_optional(&self.pool).await
    }

    pub async fn backlog_sent(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE morning_offers o SET backlog_sent=true FROM morning_offers delivered
            WHERE delivered.id=$1 AND o.user_id=delivered.user_id
                AND (o.due_at AT TIME ZONE o.timezone)::date
                    = (delivered.due_at AT TIME ZONE delivered.timezone)::date",
        )
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
            let instant = super::maintenance::now(&mut tx).await?;
            // Preserve replaced expired picks before accepting the new list.
            let tasks: Vec<i64> = sqlx::query_scalar("SELECT DISTINCT task_id FROM morning_offer_items WHERE offer_id=$1 ORDER BY task_id")
                .bind(id).fetch_all(&mut *tx).await?;
            for task in tasks {
                super::maintenance::expire_picks(
                    &mut tx,
                    "daily_tasks",
                    Some(user_id),
                    Some(task),
                    instant,
                )
                .await?;
            }
            let reset =
                crate::planning::daily_reset(super::maintenance::timezone(&timezone)?, instant);
            count = sqlx::query(
                "INSERT INTO daily_tasks(task_id, subtask_id, expires_at, timezone)
                  SELECT t.id, i.subtask_id, $4, $3
                 FROM morning_offer_items i JOIN tasks t ON t.id = i.task_id JOIN categories c ON c.id = t.category_id
                 WHERE i.offer_id = $1 AND c.user_id = $2 AND NOT t.completed
                   AND (i.subtask_id = '' OR EXISTS (SELECT 1 FROM jsonb_array_elements(t.subtasks) s
                       WHERE s->>'id' = i.subtask_id AND NOT (s->>'completed')::boolean))
                 ORDER BY t.id, i.subtask_id
                 ON CONFLICT(task_id, subtask_id) DO UPDATE SET expires_at = EXCLUDED.expires_at, timezone = EXCLUDED.timezone
                 WHERE daily_tasks.expires_at <= statement_timestamp()")
                .bind(id).bind(user_id).bind(timezone).bind(reset).execute(&mut *tx).await?.rows_affected();
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
            let reminders: Vec<(
                i64,
                chrono::DateTime<chrono::Utc>,
                String,
                chrono::DateTime<chrono::Utc>,
            )> = sqlx::query_as(
                "SELECT r.id,r.due_at,r.timezone,m.valid_until FROM reminder_tasks r
                 JOIN morning_offer_reminder_items i ON i.reminder_id=r.id
                 JOIN morning_offers m ON m.id=i.offer_id
                 WHERE i.offer_id=$1 AND i.kind='today' AND r.user_id=$2 AND NOT r.completed
                     AND r.due_at=i.due_at AND r.timezone=i.timezone AND r.selected_until IS NULL
                 ORDER BY r.id FOR UPDATE OF r",
            )
            .bind(id)
            .bind(user_id)
            .fetch_all(&mut *tx)
            .await?;
            for (reminder, event, timezone, until) in reminders {
                let zone = super::maintenance::timezone(&timezone)?;
                let expiry = crate::planning::at(
                    zone,
                    event.with_timezone(&zone).date_naive().succ_opt().unwrap(),
                    2,
                )
                .min(until);
                count += sqlx::query("UPDATE reminder_tasks SET selected_until=$2 WHERE id=$1")
                    .bind(reminder)
                    .bind(expiry)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            }
        }
        sqlx::query("UPDATE morning_offers SET state = $2, backlog_ready = true WHERE id = $1")
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
        sqlx::query_scalar(
            "SELECT COALESCE((SELECT next_update_id FROM bot_poll_offsets WHERE bot_id = $1), 0)",
        )
        .bind(self.bot_id)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn advance(&self, offset: i64) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO bot_poll_offsets (bot_id, next_update_id) VALUES ($1, $2) ON CONFLICT (bot_id) DO UPDATE SET next_update_id = GREATEST(bot_poll_offsets.next_update_id, EXCLUDED.next_update_id)")
            .bind(self.bot_id)
            .bind(offset)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
