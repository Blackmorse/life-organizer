mod support;

use backend::{
    dao::{PgReminderDao, PgUserDao, reminder_alert::ReminderAlertStore},
    domain::ReminderTaskInput,
    planning,
    reminder_alerts::deliver_at,
    service::{MessageSender, NotificationError, ReminderService},
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use std::sync::{Arc, Mutex};
use support::{TELEGRAM_ID, TestDb};

#[derive(Default)]
struct Sender {
    messages: Mutex<Vec<(i64, Vec<String>)>>,
    fail_recipient: Mutex<Option<i64>>,
}

#[async_trait::async_trait]
impl MessageSender for Sender {
    async fn send(&self, recipient: i64, messages: &[String]) -> Result<(), NotificationError> {
        if *self.fail_recipient.lock().unwrap() == Some(recipient) {
            return Err(NotificationError::ChatUnavailable);
        }
        self.messages
            .lock()
            .unwrap()
            .push((recipient, messages.to_vec()));
        Ok(())
    }
}

fn service(db: &TestDb, owner: i64) -> ReminderService {
    ReminderService::new(Arc::new(PgReminderDao::new(db.pool.clone(), owner)))
}

fn input(zone: chrono_tz::Tz, day: &str, hour: u32, all_day: bool) -> ReminderTaskInput {
    ReminderTaskInput {
        description: "Dentist <&> 😀".into(),
        detailed_description: "Private notes never sent".into(),
        due_at: planning::at(zone, day.parse().unwrap(), hour).timestamp_millis(),
        all_day,
        timezone: zone.name().into(),
    }
}

fn time(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

#[tokio::test]
async fn all_day_alerts_follow_saved_local_slots_without_daily_selection() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let reminder = svc
        .save(
            None,
            input(chrono_tz::Europe::Berlin, "2030-03-31", 0, true),
        )
        .await
        .unwrap();
    let sender = Sender::default();
    for (instant, count) in [
        ("2030-03-31T18:59:59Z", 0),
        ("2030-03-31T19:00:00Z", 1), // 21:00 after DST starts.
        ("2030-03-31T19:00:30Z", 1),
        ("2030-03-31T20:59:59Z", 1),
        ("2030-03-31T21:00:00Z", 2),
        ("2030-03-31T21:54:59Z", 2),
        ("2030-03-31T21:55:00Z", 3),
        ("2030-03-31T21:59:00Z", 3),
        ("2030-03-31T22:00:00Z", 3),
        ("2030-04-01T19:00:00Z", 3),
    ] {
        deliver_at(&db.pool, &sender, time(instant)).await.unwrap();
        assert_eq!(sender.messages.lock().unwrap().len(), count, "{instant}");
    }
    let messages = sender.messages.lock().unwrap().clone();
    for (recipient, parts) in messages.iter() {
        assert_eq!(*recipient, TELEGRAM_ID);
        let text = parts.concat();
        assert!(text.contains("Dentist &lt;&amp;&gt; 😀"));
        assert!(text.contains("2030-03-31 · All day (Europe/Berlin)"));
        assert!(!text.contains("Private notes"));
    }
    assert!(
        svc.list()
            .await
            .unwrap()
            .iter()
            .find(|r| r.id == reminder.id)
            .unwrap()
            .expires_at
            .is_none()
    );
}

#[tokio::test]
async fn timed_alert_is_one_elapsed_hour_before_due_even_across_midnight_and_dst() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    // 00:30 in UTC+14: notify on the preceding local calendar date.
    let mut value = input(chrono_tz::Pacific::Kiritimati, "2030-01-16", 0, false);
    value.due_at += Duration::minutes(30).num_milliseconds();
    svc.save(None, value).await.unwrap();
    let sender = Sender::default();
    for (instant, count) in [
        ("2030-01-15T09:29:59Z", 0),
        ("2030-01-15T09:30:00Z", 1),
        ("2030-01-15T10:29:59Z", 1),
        ("2030-01-15T10:30:00Z", 1),
    ] {
        deliver_at(&db.pool, &sender, time(instant)).await.unwrap();
        assert_eq!(sender.messages.lock().unwrap().len(), count);
    }
    assert!(
        sender.messages.lock().unwrap()[0]
            .1
            .concat()
            .contains("2030-01-16 00:30 (Pacific/Kiritimati)")
    );
    // Berlin jumps from 01:59 to 03:00: one hour before 03:00 is 01:00 local.
    let value = input(chrono_tz::Europe::Berlin, "2030-03-31", 3, false);
    svc.save(None, value).await.unwrap();
    deliver_at(&db.pool, &sender, time("2030-03-31T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn restart_catches_only_current_window_and_never_expired_or_completed_work() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    svc.save(None, input(chrono_tz::UTC, "2030-01-15", 0, true))
        .await
        .unwrap();
    let completed = svc
        .save(None, input(chrono_tz::UTC, "2030-01-15", 0, true))
        .await
        .unwrap();
    svc.complete(completed.id, true).await.unwrap();
    svc.save(None, input(chrono_tz::UTC, "2030-01-15", 23, false))
        .await
        .unwrap();
    let sender = Sender::default();
    deliver_at(&db.pool, &sender, time("2030-01-15T23:57:00Z"))
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
    // New store simulates restart, using persisted success state.
    deliver_at(&db.pool, &sender, time("2030-01-15T23:58:00Z"))
        .await
        .unwrap();
    deliver_at(&db.pool, &sender, time("2030-01-16T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn failure_retries_after_lease_without_blocking_other_users_and_uses_live_name() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(42)
        .await
        .unwrap();
    let mut value = input(chrono_tz::UTC, "2030-01-15", 23, false);
    let reminder = svc.save(None, value.clone()).await.unwrap();
    service(&db, other.id)
        .save(None, value.clone())
        .await
        .unwrap();
    let sender = Sender::default();
    *sender.fail_recipient.lock().unwrap() = Some(TELEGRAM_ID);
    let now = time("2030-01-15T22:00:00Z");
    assert!(deliver_at(&db.pool, &sender, now).await.is_err());
    assert_eq!(sender.messages.lock().unwrap()[0].0, 42);
    *sender.fail_recipient.lock().unwrap() = None;
    value.description = "Renamed <task>".into();
    svc.save(Some(reminder.id), value).await.unwrap();
    deliver_at(&db.pool, &sender, now + Duration::seconds(30))
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
    deliver_at(&db.pool, &sender, now + Duration::minutes(6))
        .await
        .unwrap();
    let messages = sender.messages.lock().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].0, TELEGRAM_ID);
    assert!(messages[1].1.concat().contains("Renamed &lt;task&gt;"));
}

#[tokio::test]
async fn pending_alerts_recheck_completion_deletion_reschedule_and_time_mode() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let value = input(chrono_tz::UTC, "2030-01-15", 22, false);
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(svc.save(None, value.clone()).await.unwrap().id);
    }
    let sender = Sender::default();
    *sender.fail_recipient.lock().unwrap() = Some(TELEGRAM_ID);
    let now = time("2030-01-15T21:00:00Z");
    assert!(deliver_at(&db.pool, &sender, now).await.is_err());
    svc.complete(ids[0], true).await.unwrap();
    svc.delete(ids[1]).await.unwrap();
    svc.save(Some(ids[2]), input(chrono_tz::UTC, "2030-01-16", 22, false))
        .await
        .unwrap();
    svc.save(
        Some(ids[3]),
        ReminderTaskInput {
            all_day: true,
            ..value.clone()
        },
    )
    .await
    .unwrap();
    svc.save(
        Some(ids[4]),
        ReminderTaskInput {
            timezone: "Europe/Berlin".into(),
            ..value
        },
    )
    .await
    .unwrap();
    *sender.fail_recipient.lock().unwrap() = None;
    deliver_at(&db.pool, &sender, now + Duration::minutes(6))
        .await
        .unwrap();
    // Only NEW identities send: the all-day slot and the changed timezone.
    // None of the five stale timed jobs can send.
    assert_eq!(sender.messages.lock().unwrap().len(), 2);
    assert!(
        sender.messages.lock().unwrap()[0]
            .1
            .concat()
            .contains("All day")
    );
    assert!(
        sender.messages.lock().unwrap()[1]
            .1
            .concat()
            .contains("Europe/Berlin")
    );
    // Completion suppresses remaining all-day slots.
    svc.complete(ids[3], true).await.unwrap();
    deliver_at(&db.pool, &sender, time("2030-01-15T23:55:00Z"))
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 2);
    let links: i64 =
        sqlx::query_scalar("SELECT count(*) FROM reminder_alerts WHERE reminder_id=$1")
            .bind(ids[1])
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(links, 0);
}

#[tokio::test]
async fn concurrent_claims_and_stale_acknowledgements_respect_durable_leases() {
    let db = TestDb::new().await;
    service(&db, db.user.id)
        .save(None, input(chrono_tz::UTC, "2030-01-15", 23, false))
        .await
        .unwrap();
    let store = ReminderAlertStore::new(db.pool.clone());
    let now = time("2030-01-15T22:00:00Z");
    store.prepare_at(now).await.unwrap();
    let (a, b) = tokio::join!(store.claim_at(now), store.claim_at(now));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(usize::from(a.is_some()) + usize::from(b.is_some()), 1);
    let old = a.or(b).unwrap();
    let retried = store
        .claim_at(now + Duration::minutes(5))
        .await
        .unwrap()
        .unwrap();
    store.sent(&old).await.unwrap();
    let sent: bool = sqlx::query_scalar("SELECT sent FROM reminder_alerts WHERE id=$1")
        .bind(old.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(!sent);
    store.sent(&retried).await.unwrap();
    assert!(
        store
            .claim_at(now + Duration::minutes(10))
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn all_day_windows_use_local_dates_on_dst_days_and_extreme_timezones() {
    for zone in [
        chrono_tz::Europe::Berlin,
        chrono_tz::Pacific::Kiritimati,
        chrono_tz::America::Los_Angeles,
    ] {
        for day in ["2030-03-31", "2030-10-27"] {
            let day: NaiveDate = day.parse().unwrap();
            let due = planning::at(zone, day, 0);
            for (slot, hour, minute) in [(1, 21, 0), (2, 23, 0), (3, 23, 55)] {
                let start =
                    planning::local_instant(zone, day.and_hms_opt(hour, minute, 0).unwrap());
                let (actual, _, end) =
                    planning::reminder_alert_window(zone, due, true, start).unwrap();
                assert_eq!(actual, slot);
                assert!(end > start);
            }
            let midnight = planning::at(zone, day.succ_opt().unwrap(), 0);
            assert!(planning::reminder_alert_window(zone, due, true, midnight).is_none());
            assert!(
                planning::reminder_alert_window(zone, due, true, planning::at(zone, day, 20))
                    .is_none()
            );
        }
    }
}

#[tokio::test]
async fn final_slot_can_retry_before_midnight_but_never_after_it() {
    let db = TestDb::new().await;
    service(&db, db.user.id)
        .save(None, input(chrono_tz::UTC, "2030-01-15", 0, true))
        .await
        .unwrap();
    let sender = Sender::default();
    *sender.fail_recipient.lock().unwrap() = Some(TELEGRAM_ID);
    assert!(
        deliver_at(&db.pool, &sender, time("2030-01-15T23:55:00Z"))
            .await
            .is_err()
    );
    *sender.fail_recipient.lock().unwrap() = None;
    deliver_at(&db.pool, &sender, time("2030-01-15T23:56:30Z"))
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 1);

    service(&db, db.user.id)
        .save(None, input(chrono_tz::UTC, "2030-01-15", 0, true))
        .await
        .unwrap();
    *sender.fail_recipient.lock().unwrap() = Some(TELEGRAM_ID);
    assert!(
        deliver_at(&db.pool, &sender, time("2030-01-15T23:59:30Z"))
            .await
            .is_err()
    );
    *sender.fail_recipient.lock().unwrap() = None;
    deliver_at(&db.pool, &sender, time("2030-01-16T00:01:00Z"))
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn migration_preserves_existing_reminder_progress_and_selection() {
    let db = TestDb::new_at_version(19).await;
    let svc = service(&db, db.user.id);
    for all_day in [false, true] {
        let reminder = svc
            .save(
                None,
                input(chrono_tz::Europe::Berlin, "2030-01-15", 15, all_day),
            )
            .await
            .unwrap();
        sqlx::query("UPDATE reminder_tasks SET completed=TRUE,selected_until='2030-01-16T01:00:00Z' WHERE id=$1")
            .bind(reminder.id).execute(&db.pool).await.unwrap();
    }
    let snapshot = || async {
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT to_jsonb(r) FROM reminder_tasks r ORDER BY id",
        )
        .fetch_all(&db.pool)
        .await
        .unwrap()
    };
    let before = snapshot().await;
    backend::db::MIGRATOR.run(&db.pool).await.unwrap();
    assert_eq!(snapshot().await, before);
    let alerts: i64 = sqlx::query_scalar("SELECT count(*) FROM reminder_alerts")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(alerts, 0);
}
