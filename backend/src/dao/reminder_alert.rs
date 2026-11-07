//! Durable alert leases with live reminder identity/completion checks.
use crate::planning;
use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, PgPool};

#[derive(FromRow)]
struct Reminder {
    id: i64,
    due_at: DateTime<Utc>,
    timezone: String,
    all_day: bool,
}

#[derive(FromRow)]
pub struct ReminderAlert {
    pub id: i64,
    pub recipient: i64,
    pub description: String,
    pub due_at: DateTime<Utc>,
    pub timezone: String,
    pub all_day: bool,
    pub retry_at: DateTime<Utc>,
}

pub struct ReminderAlertStore {
    pool: PgPool,
}

impl ReminderAlertStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn prepare_at(&self, now: DateTime<Utc>) -> Result<(), sqlx::Error> {
        let reminders = sqlx::query_as::<_, Reminder>(
            "SELECT id, due_at, timezone, all_day FROM reminder_tasks WHERE NOT completed",
        )
        .fetch_all(&self.pool)
        .await?;
        for reminder in reminders {
            let zone = planning::zone(&reminder.timezone)
                .map_err(|_| sqlx::Error::Protocol("invalid saved reminder timezone".into()))?;
            let Some((slot, notify_at, valid_until)) =
                planning::reminder_alert_window(zone, reminder.due_at, reminder.all_day, now)
            else {
                continue;
            };
            // The reminder may have been deleted/edited after the scan. Recheck
            // in the insert itself rather than routinely creating stale jobs.
            sqlx::query("INSERT INTO reminder_alerts (reminder_id,due_at,timezone,all_day,slot,notify_at,valid_until) SELECT id,due_at,timezone,all_day,$5,$6,$7 FROM reminder_tasks WHERE id=$1 AND due_at=$2 AND timezone=$3 AND all_day=$4 AND NOT completed ON CONFLICT DO NOTHING")
                .bind(reminder.id).bind(reminder.due_at).bind(reminder.timezone)
                .bind(reminder.all_day).bind(slot).bind(notify_at).bind(valid_until)
                .execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn claim_at(&self, now: DateTime<Utc>) -> Result<Option<ReminderAlert>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let alert = sqlx::query_as::<_, ReminderAlert>(
            "SELECT a.id,u.telegram_id AS recipient,r.description,r.due_at,r.timezone,r.all_day,COALESCE(a.retry_at,$1) AS retry_at FROM reminder_alerts a JOIN reminder_tasks r ON r.id=a.reminder_id AND r.due_at=a.due_at AND r.timezone=a.timezone AND r.all_day=a.all_day JOIN users u ON u.id=r.user_id WHERE NOT a.sent AND NOT r.completed AND a.notify_at <= $1 AND a.valid_until > $1 AND (a.retry_at IS NULL OR a.retry_at <= $1) ORDER BY a.notify_at,a.id LIMIT 1 FOR UPDATE OF a,r SKIP LOCKED",
        ).bind(now).fetch_optional(&mut *tx).await?;
        let Some(mut alert) = alert else {
            return Ok(None);
        };
        // Short enough to retry the final all-day slot before midnight.
        alert.retry_at = now + Duration::minutes(1);
        sqlx::query("UPDATE reminder_alerts SET retry_at=$2 WHERE id=$1")
            .bind(alert.id)
            .bind(alert.retry_at)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(alert))
    }

    pub async fn sent(&self, alert: &ReminderAlert) -> Result<(), sqlx::Error> {
        // A superseded lease cannot acknowledge a newer delivery attempt.
        sqlx::query("UPDATE reminder_alerts SET sent=TRUE WHERE id=$1 AND retry_at=$2")
            .bind(alert.id)
            .bind(alert.retry_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
