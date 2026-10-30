mod support;

use backend::{
    dao::{
        DailyDao, PgDailyDao, PgReminderDao, PgUserDao,
        morning::{Decision, MorningStore},
    },
    domain::ReminderTaskInput,
    morning::{BotUpdate, MorningBot, MorningWorker},
    service::{MessageSender, NotificationError, ReminderService},
};
use std::sync::{Arc, Mutex};
use support::{TELEGRAM_ID, TestDb};

fn service(db: &TestDb, owner: i64) -> ReminderService {
    ReminderService::new(Arc::new(PgReminderDao::new(db.pool.clone(), owner)))
}
async fn input(db: &TestDb, days: i32) -> ReminderTaskInput {
    let due_at = sqlx::query_scalar("SELECT (extract(epoch FROM (current_date + $1 + time '15:30') AT TIME ZONE 'UTC')*1000)::bigint")
        .bind(days).fetch_one(&db.pool).await.unwrap();
    ReminderTaskInput {
        description: "Dentist <&>".into(),
        due_at,
        timezone: "UTC".into(),
    }
}
async fn prepare_today(db: &TestDb) -> i64 {
    // Preparation uses an explicit instant; delivery is forced due to avoid wall-clock-dependent tests.
    sqlx::query(
        "SELECT prepare_reminder_morning_offers((current_date + time '08:00') AT TIME ZONE 'UTC')",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query_scalar("UPDATE morning_offers SET due_at=statement_timestamp()-interval '1 second', valid_until=statement_timestamp()+interval '1 hour' RETURNING id")
        .fetch_one(&db.pool).await.unwrap()
}

#[tokio::test]
async fn reminder_crud_validation_ownership_and_exact_instant() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let mut value = input(&db, 1).await;
    value.due_at += 123;
    let saved = svc.save(None, value.clone()).await.unwrap();
    assert_eq!(saved.details, value);
    assert!(!saved.completed);
    assert_eq!(saved.expires_at, None);
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(42)
        .await
        .unwrap();
    let outsider = service(&db, other.id);
    assert!(outsider.list().await.unwrap().is_empty());
    assert!(outsider.save(Some(saved.id), value.clone()).await.is_err());
    assert!(outsider.complete(saved.id, true).await.is_err());
    assert!(outsider.dismiss(saved.id).await.is_err());
    assert!(outsider.delete(saved.id).await.is_err());
    for invalid in [
        ReminderTaskInput {
            description: " \n".into(),
            ..value.clone()
        },
        ReminderTaskInput {
            timezone: "Mars/Olympus".into(),
            ..value.clone()
        },
        ReminderTaskInput {
            due_at: i64::MAX,
            ..value.clone()
        },
    ] {
        assert!(svc.save(None, invalid).await.is_err());
    }
    value.description = "Updated".into();
    assert_eq!(
        svc.save(Some(saved.id), value)
            .await
            .unwrap()
            .details
            .description,
        "Updated"
    );
    assert!(svc.complete(saved.id, true).await.unwrap().completed);
    svc.delete(saved.id).await.unwrap();
    assert!(svc.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn tomorrow_notice_never_selects_and_today_acceptance_is_owned_atomic_and_idempotent() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let today = svc.save(None, input(&db, 0).await).await.unwrap();
    let tomorrow = svc.save(None, input(&db, 1).await).await.unwrap();
    let id = prepare_today(&db).await;
    let store = MorningStore::new(db.pool.clone());
    let offer = store.claim().await.unwrap().unwrap();
    assert_eq!(offer.items.len(), 1);
    assert_eq!(offer.tomorrow.len(), 1);
    assert!(offer.items[0].task.contains("15:30 (UTC)"));
    assert!(
        PgDailyDao::new(db.pool.clone(), db.user.id)
            .summary()
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.decide(id, TELEGRAM_ID + 1, true).await.unwrap(),
        Decision::Unavailable
    );
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Accepted(1)
    );
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Unavailable
    );
    let all = svc.list().await.unwrap();
    assert!(
        all.iter()
            .find(|r| r.id == today.id)
            .unwrap()
            .expires_at
            .is_some()
    );
    assert_eq!(
        all.iter().find(|r| r.id == tomorrow.id).unwrap().expires_at,
        None
    );
    assert!(svc.complete(today.id, true).await.unwrap().completed);
    let daily = PgDailyDao::new(db.pool.clone(), db.user.id)
        .summary()
        .await
        .unwrap();
    assert_eq!(daily.len(), 1);
    assert!(daily[0].completed);
    assert_eq!(daily[0].category, "⏰ Reminders");
    svc.dismiss(today.id).await.unwrap();
    assert!(
        PgDailyDao::new(db.pool.clone(), db.user.id)
            .summary()
            .await
            .unwrap()
            .is_empty()
    );
    store.sweep().await.unwrap();
    assert!(store.claim().await.unwrap().is_none());
}

#[tokio::test]
async fn stale_deleted_completed_and_late_reminders_do_not_get_selected() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let changed = svc.save(None, input(&db, 0).await).await.unwrap();
    let deleted = svc.save(None, input(&db, 0).await).await.unwrap();
    let completed = svc.save(None, input(&db, 0).await).await.unwrap();
    let id = prepare_today(&db).await;
    let store = MorningStore::new(db.pool.clone());
    store.sent(id).await.unwrap();
    svc.save(Some(changed.id), input(&db, 2).await)
        .await
        .unwrap();
    svc.delete(deleted.id).await.unwrap();
    svc.complete(completed.id, true).await.unwrap();
    let late = svc.save(None, input(&db, 0).await).await.unwrap();
    store.sweep().await.unwrap();
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Accepted(0)
    );
    assert!(
        svc.list()
            .await
            .unwrap()
            .iter()
            .all(|r| r.expires_at.is_none())
    );
    assert!(svc.list().await.unwrap().iter().any(|r| r.id == late.id));
    assert!(
        PgDailyDao::new(db.pool.clone(), db.user.id)
            .summary()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn declining_and_expiry_preserve_reminders_without_carryover_or_missed_day_backfill() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let task = svc.save(None, input(&db, 0).await).await.unwrap();
    let id = prepare_today(&db).await;
    let store = MorningStore::new(db.pool.clone());
    assert_eq!(
        store.decide(id, TELEGRAM_ID, false).await.unwrap(),
        Decision::Declined
    );
    assert_eq!(svc.list().await.unwrap()[0].expires_at, None);
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Unavailable
    );

    // Model an unanswered expired prompt; the callback cannot add it afterward.
    sqlx::query("UPDATE morning_offers SET state='sent', valid_until=statement_timestamp()-interval '1 second'").execute(&db.pool).await.unwrap();
    assert_eq!(
        store.decide(id, TELEGRAM_ID, true).await.unwrap(),
        Decision::Unavailable
    );
    sqlx::query("UPDATE reminder_tasks SET selected_until=statement_timestamp()-interval '1 second', completed=true WHERE id=$1")
        .bind(task.id).execute(&db.pool).await.unwrap();
    assert!(
        PgDailyDao::new(db.pool.clone(), db.user.id)
            .summary()
            .await
            .unwrap()
            .is_empty()
    );
    let saved = svc.list().await.unwrap().pop().unwrap();
    assert!(saved.completed);
    assert_eq!(saved.details, task.details);
    svc.complete(task.id, false).await.unwrap();
    sqlx::query("SELECT prepare_reminder_morning_offers(statement_timestamp()+interval '3 days')")
        .execute(&db.pool)
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM morning_offer_reminder_items WHERE kind='today'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    assert!(
        PgDailyDao::new(db.pool.clone(), db.user.id)
            .summary()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn mixed_morning_offer_and_summary_keep_namespaces_separate_and_rescheduling_clears_selection()
 {
    use backend::{
        dao::{CategoryDao, TaskDao},
        domain::TaskUrgency,
        service::DailyService,
    };
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let reminder = svc.save(None, input(&db, 0).await).await.unwrap();
    svc.save(None, input(&db, 1).await).await.unwrap();
    let offer = prepare_today(&db).await;
    let category = db.categories().create("Work").await.unwrap();
    let ordinary = db
        .tasks()
        .create(category.id, "Ordinary task", TaskUrgency::Soon)
        .await
        .unwrap();
    sqlx::query("INSERT INTO morning_offer_items(offer_id,task_id,subtask_id) VALUES ($1,$2,'')")
        .bind(offer)
        .bind(ordinary.id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("WITH template AS (INSERT INTO scheduled_tasks(user_id,description,weekdays,timezone,created_at) VALUES ($1,'Recurring task',ARRAY[1,2,3,4,5,6,7]::smallint[],'UTC',statement_timestamp()+interval '1 day') RETURNING id), occurrence AS (INSERT INTO scheduled_occurrences(scheduled_task_id,planning_date,description,timezone,expires_at,subtasks) SELECT id,current_date,'Recurring task','UTC',statement_timestamp()+interval '1 hour','[]'::jsonb FROM template RETURNING id) INSERT INTO morning_offer_scheduled_items(offer_id,occurrence_id) SELECT $2,id FROM occurrence")
        .bind(db.user.id).bind(offer).execute(&db.pool).await.unwrap();
    let bot = Arc::new(Bot::default());
    MorningWorker::new(db.pool.clone(), bot.clone())
        .deliver_due()
        .await
        .unwrap();
    let prompts = bot.offers.lock().unwrap().clone();
    assert_eq!(prompts.len(), 1);
    let text = prompts[0].concat();
    for part in [
        "tomorrow",
        "Ordinary task",
        "Recurring task",
        "15:30 (UTC)",
        "Add these tasks",
    ] {
        assert!(text.contains(part), "missing {part}");
    }
    assert_eq!(
        MorningStore::new(db.pool.clone())
            .decide(offer, TELEGRAM_ID, true)
            .await
            .unwrap(),
        Decision::Accepted(3)
    );
    let messages = DailyService::new(Arc::new(PgDailyDao::new(db.pool.clone(), db.user.id)))
        .current_messages()
        .await
        .unwrap()
        .concat();
    for heading in ["📁 Work", "🔁 Scheduled", "⏰ Reminders"] {
        assert_eq!(messages.matches(heading).count(), 1);
    }
    let selected = svc
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.id == reminder.id)
        .unwrap();
    let mut details = selected.details.clone();
    details.description = "Renamed reminder".into();
    assert_eq!(
        svc.save(Some(reminder.id), details.clone())
            .await
            .unwrap()
            .expires_at,
        selected.expires_at
    );
    details.timezone = "Europe/Berlin".into();
    assert_eq!(
        svc.save(Some(reminder.id), details)
            .await
            .unwrap()
            .expires_at,
        None
    );
    let rows = PgDailyDao::new(db.pool.clone(), db.user.id)
        .summary()
        .await
        .unwrap();
    assert!(
        rows.iter()
            .all(|r| r.source != backend::domain::DailyTaskSource::Reminder)
    );
}

#[tokio::test]
async fn morning_dates_use_calendar_days_not_24_hours_or_two_am_planning_dates() {
    let db = TestDb::new().await;
    for (zone, event, instant, tomorrow_due, today_due, expiry) in [
        (
            "America/New_York",
            "2026-03-08T05:30:00Z",
            "2026-03-07T14:00:00Z",
            "2026-03-07T13:00:00Z",
            "2026-03-08T12:00:00Z",
            "2026-03-09T06:00:00Z",
        ),
        (
            "Asia/Kathmandu",
            "2026-10-31T18:45:00Z",
            "2026-10-31T04:00:00Z",
            "2026-10-31T02:15:00Z",
            "2026-11-01T02:15:00Z",
            "2026-11-01T20:15:00Z",
        ),
    ] {
        let id: i64 = sqlx::query_scalar("INSERT INTO reminder_tasks(user_id, description, due_at, timezone) VALUES ($1,'Early event',$2::text::timestamptz,$3) RETURNING id")
            .bind(db.user.id).bind(event).bind(zone).fetch_one(&db.pool).await.unwrap();
        sqlx::query("SELECT prepare_reminder_morning_offers($1::text::timestamptz)")
            .bind(instant)
            .execute(&db.pool)
            .await
            .unwrap();
        let correct: bool = sqlx::query_scalar("SELECT m.due_at=$2::text::timestamptz FROM morning_offer_reminder_items i JOIN morning_offers m ON m.id=i.offer_id WHERE reminder_id=$1 AND kind='tomorrow'")
            .bind(id).bind(tomorrow_due).fetch_one(&db.pool).await.unwrap();
        assert!(correct);
        sqlx::query("SELECT prepare_reminder_morning_offers($1::text::timestamptz)")
            .bind(today_due)
            .execute(&db.pool)
            .await
            .unwrap();
        let correct: bool = sqlx::query_scalar("SELECT m.due_at=$2::text::timestamptz AND m.valid_until=$3::text::timestamptz FROM morning_offer_reminder_items i JOIN morning_offers m ON m.id=i.offer_id WHERE reminder_id=$1 AND kind='today'")
            .bind(id).bind(today_due).bind(expiry).fetch_one(&db.pool).await.unwrap();
        assert!(correct);
    }
}

#[derive(Default)]
struct Bot {
    messages: Mutex<Vec<String>>,
    offers: Mutex<Vec<Vec<String>>>,
}
#[async_trait::async_trait]
impl MessageSender for Bot {
    async fn send(&self, _: i64, messages: &[String]) -> Result<(), NotificationError> {
        self.messages.lock().unwrap().extend_from_slice(messages);
        Ok(())
    }
}
#[async_trait::async_trait]
impl MorningBot for Bot {
    async fn configure_commands(&self) -> Result<(), NotificationError> {
        Ok(())
    }
    async fn offer(&self, _: i64, _: i64, messages: &[String]) -> Result<(), NotificationError> {
        self.offers.lock().unwrap().push(messages.to_vec());
        Ok(())
    }
    async fn updates(&self, _: i64) -> Result<Vec<BotUpdate>, NotificationError> {
        Ok(vec![])
    }
    async fn answer(&self, _: &str, _: &str) -> Result<(), NotificationError> {
        Ok(())
    }
}

#[tokio::test]
async fn tomorrow_only_delivery_has_no_yes_no_prompt_and_is_not_repeated() {
    let db = TestDb::new().await;
    service(&db, db.user.id)
        .save(None, input(&db, 1).await)
        .await
        .unwrap();
    prepare_today(&db).await;
    let bot = Arc::new(Bot::default());
    let worker = MorningWorker::new(db.pool.clone(), bot.clone());
    worker.deliver_due().await.unwrap();
    let messages = bot.messages.lock().unwrap().concat();
    assert!(messages.contains("tomorrow there are scheduled one-time tasks"));
    assert!(messages.contains("Dentist &lt;&amp;&gt;"));
    assert!(messages.contains("15:30 (UTC)"));
    assert!(!messages.contains("Add these tasks"));
    assert!(bot.offers.lock().unwrap().is_empty());
    worker.deliver_due().await.unwrap();
    assert_eq!(bot.messages.lock().unwrap().concat(), messages);
}

#[tokio::test]
async fn api_requires_exact_time_rejects_urgency_and_scopes_mutations() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let db = TestDb::new().await;
    let app = db.app();
    let value = input(&db, 1).await;
    let request = |method: &str, path: &str, body: serde_json::Value, user| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("Authorization", support::authorization_for(user))
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    for body in [
        serde_json::json!({"description":"Missing time","timezone":"UTC"}),
        serde_json::json!({"description":"Unknown urgency","due_at":value.due_at,"timezone":"UTC","urgency":"soon"}),
    ] {
        let response = app
            .clone()
            .oneshot(request("POST", "/reminder-tasks", body, TELEGRAM_ID))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            "/reminder-tasks",
            serde_json::to_value(value).unwrap(),
            TELEGRAM_ID,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let saved: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(saved.get("urgency").is_none());
    let id = saved["id"].as_i64().unwrap();
    let response = app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/reminder-tasks/{id}/completion"),
            serde_json::json!({"completed":true}),
            TELEGRAM_ID + 1,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .oneshot(request(
            "PUT",
            &format!("/reminder-tasks/{id}/completion"),
            serde_json::json!({"completed":true}),
            TELEGRAM_ID,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
