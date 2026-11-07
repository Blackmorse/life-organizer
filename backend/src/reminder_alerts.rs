//! Reminder notifications are independent of daily selection and morning offers.
use crate::{
    dao::reminder_alert::ReminderAlertStore,
    service::{MessageSender, NotificationError, reminder_alert_messages},
};
use chrono::{DateTime, Utc};
use sqlx::PgPool;

pub async fn deliver_at(
    pool: &PgPool,
    sender: &(impl MessageSender + ?Sized),
    now: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let started = std::time::Instant::now();
    let store = ReminderAlertStore::new(pool.clone());
    store.prepare_at(now).await?;
    let mut failed = false;
    // A blocked recipient must not prevent alerts to other users in this batch.
    for _ in 0..20 {
        let current = now + chrono::Duration::from_std(started.elapsed())?;
        let Some(alert) = store.claim_at(current).await? else {
            break;
        };
        let messages = reminder_alert_messages(&alert);
        if sender.send(alert.recipient, &messages).await.is_ok() {
            store.sent(&alert).await?;
        } else {
            failed = true;
        }
    }
    if failed {
        return Err(NotificationError::DeliveryFailed.into());
    }
    Ok(())
}
