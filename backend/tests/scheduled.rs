mod support;

use backend::{
    dao::{DailyDao, PgDailyDao, PgScheduledDao, PgUserDao},
    domain::{ScheduledTaskInput, Subtask},
    service::ScheduledService,
};
use std::sync::Arc;
use support::TestDb;

#[derive(Default)]
struct Sender(std::sync::Mutex<Vec<String>>);
#[async_trait::async_trait]
impl backend::service::MessageSender for Sender {
    async fn send(
        &self,
        _: i64,
        messages: &[String],
    ) -> Result<(), backend::service::NotificationError> {
        self.0.lock().unwrap().extend_from_slice(messages);
        Ok(())
    }
}

fn input() -> ScheduledTaskInput {
    ScheduledTaskInput {
        description: "Sport exercises".into(),
        detailed_description: String::new(),
        weekdays: vec![1, 2, 3, 4, 5, 6, 7],
        timezone: "Europe/Berlin".into(),
        subtasks: vec![Subtask::new("Warm up"), Subtask::new("Run")],
    }
}
fn service(db: &TestDb, user: i64) -> ScheduledService {
    ScheduledService::new(Arc::new(PgScheduledDao::new(db.pool.clone(), user)))
}

async fn accept_scheduled(db: &TestDb) {
    backend::dao::generate_scheduled(&db.pool, None)
        .await
        .unwrap();
    let store = backend::dao::morning::MorningStore::new(db.pool.clone());
    store.sweep().await.unwrap();
    let id: i64 = sqlx::query_scalar("UPDATE morning_offers SET due_at = statement_timestamp() - interval '1 second' RETURNING id")
        .fetch_one(&db.pool).await.unwrap();
    store.decide(id, support::TELEGRAM_ID, true).await.unwrap();
}

#[tokio::test]
async fn manual_today_selection_is_owned_idempotent_and_restores_progress_after_removal() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let task = svc.save(None, input()).await.unwrap();
    assert!(task.available_today_until.is_some());
    assert!(svc.daily().await.unwrap().is_empty());
    // An ignored or explicitly declined offer does not block manual selection.
    let store = backend::dao::morning::MorningStore::new(db.pool.clone());
    store.sweep().await.unwrap();
    sqlx::query("UPDATE morning_offers SET state='declined', backlog_ready=true")
        .execute(&db.pool)
        .await
        .unwrap();
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(123)
        .await
        .unwrap();
    assert!(service(&db, other.id).select_today(task.id).await.is_err());
    assert!(svc.select_today(0).await.is_err());
    let selected = svc.select_today(task.id).await.unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(svc.select_today(task.id).await.unwrap(), selected);
    let occurrence = &selected[0];
    let mut steps = occurrence.subtasks.clone();
    steps[0].completed = true;
    let progress = svc
        .progress(occurrence.id, None, Some(steps))
        .await
        .unwrap();
    svc.dismiss(occurrence.id).await.unwrap();
    assert!(svc.daily().await.unwrap().is_empty());
    assert!(svc.list().await.unwrap()[0].available_today_until.is_some());
    assert_eq!(svc.select_today(task.id).await.unwrap(), progress);
    svc.progress(occurrence.id, Some(true), None).await.unwrap();
    svc.dismiss(occurrence.id).await.unwrap();
    assert!(svc.select_today(task.id).await.is_err());
    assert_eq!(svc.list().await.unwrap()[0].available_today_until, None);
    assert!(!svc.list().await.unwrap()[0].details.subtasks[0].completed);
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT bool_and(state='declined') FROM morning_offers")
            .fetch_one(&db.pool)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn manual_scheduled_selection_uses_saved_planning_day_and_rejects_off_days_and_expiry() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    // Pick a real IANA zone currently before 02:00: yesterday's weekday must apply.
    let (zone, weekday): (String, i16) = sqlx::query_as("SELECT name, extract(isodow FROM (statement_timestamp() AT TIME ZONE name)::date - 1)::smallint FROM pg_timezone_names WHERE name LIKE '%/%' AND extract(hour FROM statement_timestamp() AT TIME ZONE name) < 2 LIMIT 1")
        .fetch_one(&db.pool).await.unwrap();
    let mut value = input();
    value.timezone = zone;
    value.weekdays = vec![weekday];
    let task = svc.save(None, value.clone()).await.unwrap();
    assert!(task.available_today_until.is_some());
    let selected = svc.select_today(task.id).await.unwrap();
    assert_eq!(selected[0].expires_at, task.available_today_until.unwrap());
    value.weekdays = vec![weekday % 7 + 1];
    let off_day = svc.save(None, value).await.unwrap();
    assert_eq!(off_day.available_today_until, None);
    assert!(svc.select_today(off_day.id).await.is_err());
    sqlx::query("UPDATE scheduled_occurrences SET expires_at=statement_timestamp()-interval '1 second', completed=false")
        .execute(&db.pool).await.unwrap();
    assert!(svc.select_today(task.id).await.is_err());
    assert_eq!(svc.list().await.unwrap()[0].available_today_until, None);
    assert!(svc.daily().await.unwrap().is_empty());
}

#[tokio::test]
async fn detailed_descriptions_snapshot_with_steps_and_survive_progress() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let mut value = input();
    value.detailed_description = "Template notes".into();
    value.subtasks[0].detailed_description = "Step notes".into();
    let template = svc.save(None, value.clone()).await.unwrap();
    assert_eq!(svc.list().await.unwrap()[0], template);
    accept_scheduled(&db).await;
    let occurrence = svc.daily().await.unwrap().remove(0);
    assert_eq!(occurrence.detailed_description, "Template notes");
    assert_eq!(occurrence.subtasks[0].detailed_description, "Step notes");
    value.detailed_description = "Future notes".into();
    value.subtasks[0].detailed_description.clear();
    svc.save(Some(template.id), value).await.unwrap();
    let updated = svc
        .progress(occurrence.id, Some(true), None)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(
        updated.detailed_description,
        occurrence.detailed_description
    );
    assert_eq!(updated.subtasks, occurrence.subtasks);
    let messages = PgDailyDao::new(db.pool.clone(), db.user.id)
        .summary()
        .await
        .unwrap();
    assert!(messages.iter().all(|row| {
        !row.task.contains("notes")
            && row
                .subtask
                .as_ref()
                .is_none_or(|step| !step.contains("notes"))
    }));
}

#[tokio::test]
async fn recurring_progress_is_isolated_idempotent_owned_and_dismissible() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let task = svc.save(None, input()).await.unwrap();
    assert!(svc.daily().await.unwrap().is_empty());
    assert!(
        PgDailyDao::new(db.pool.clone(), db.user.id)
            .summary()
            .await
            .unwrap()
            .is_empty()
    );
    accept_scheduled(&db).await;
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(123)
        .await
        .unwrap();
    let outsider = service(&db, other.id);
    assert!(outsider.list().await.unwrap().is_empty());
    assert!(outsider.save(Some(task.id), input()).await.is_err());
    assert!(outsider.delete(task.id).await.is_err());
    let daily = svc.daily().await.unwrap();
    assert_eq!(daily.len(), 1);
    let pick = &daily[0];
    assert_eq!(pick.scheduled_task_id, task.id);
    assert!(!pick.completed);
    assert_eq!(svc.daily().await.unwrap(), daily);
    assert!(outsider.daily().await.unwrap().is_empty());
    assert!(outsider.progress(pick.id, Some(true), None).await.is_err());
    assert!(outsider.dismiss(pick.id).await.is_err());
    let mut steps = pick.subtasks.clone();
    for step in &mut steps {
        step.completed = true;
    }
    assert!(svc.progress(pick.id, None, Some(steps)).await.unwrap()[0].completed);
    assert!(!svc.list().await.unwrap()[0].details.subtasks[0].completed);
    assert!(!svc.progress(pick.id, Some(false), None).await.unwrap()[0].completed);

    let summary = PgDailyDao::new(db.pool.clone(), db.user.id)
        .summary()
        .await
        .unwrap();
    assert_eq!(summary.len(), 2);
    assert_eq!(summary[0].task, "Sport exercises");
    assert_eq!(summary[0].subtask.as_deref(), Some("Warm up"));
    assert!(summary[0].category_id.is_none());
    assert!(summary[0].completed);
    assert_eq!(summary[1].subtask.as_deref(), Some("Run"));
    assert!(summary[1].completed);
    assert!(
        PgDailyDao::new(db.pool.clone(), other.id)
            .summary()
            .await
            .unwrap()
            .is_empty()
    );

    let mut edited = input();
    edited.description = "New exercise".into();
    svc.save(Some(task.id), edited).await.unwrap();
    assert_eq!(svc.daily().await.unwrap()[0].description, "Sport exercises");
    assert!(svc.dismiss(pick.id).await.unwrap().is_empty());
    assert!(svc.daily().await.unwrap().is_empty());
    svc.delete(task.id).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM scheduled_occurrences")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn schedules_validate_required_days_timezone_and_descriptions() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    for days in [vec![], vec![0], vec![8], vec![1, 1]] {
        let mut value = input();
        value.weekdays = days;
        assert!(svc.save(None, value).await.is_err());
    }
    for zone in ["", "Mars/Olympus"] {
        let mut value = input();
        value.timezone = zone.into();
        assert!(svc.save(None, value).await.is_err());
    }
    let mut value = input();
    value.description = "  ".into();
    assert!(svc.save(None, value).await.is_err());
    let mut value = input();
    value.subtasks[0].description = "\n".into();
    assert!(svc.save(None, value).await.is_err());
    let mut value = input();
    value.weekdays = vec![5, 1, 3];
    value.subtasks[0].completed = true;
    let saved = svc.save(None, value).await.unwrap();
    assert_eq!(saved.details.weekdays, vec![1, 3, 5]);
    assert!(!saved.details.subtasks[0].completed);
}

#[tokio::test]
async fn generation_obeys_planning_days_dst_and_resets_progress_without_backfill() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    let mut value = input();
    value.weekdays = vec![1, 3, 5];
    let task = svc.save(None, value).await.unwrap();
    sqlx::query("UPDATE scheduled_tasks SET created_at='2026-01-01Z' WHERE id=$1")
        .bind(task.id)
        .execute(&db.pool)
        .await
        .unwrap();
    // Monday's occurrence persists before Tuesday 02:00, then Tuesday is off.
    for instant in [
        "2026-10-26T10:00:00Z",
        "2026-10-27T00:59:00Z",
        "2026-10-27T01:00:00Z",
    ] {
        support::generate_at(&db, instant).await;
    }
    let dates: Vec<String> = sqlx::query_scalar(
        "SELECT planning_date::text FROM scheduled_occurrences ORDER BY planning_date",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(dates, vec!["2026-10-26"]);
    sqlx::query("UPDATE scheduled_occurrences SET completed=true")
        .execute(&db.pool)
        .await
        .unwrap();
    support::generate_at(&db, "2026-10-28T12:00:00Z").await;
    let flags: Vec<bool> =
        sqlx::query_scalar("SELECT completed FROM scheduled_occurrences ORDER BY planning_date")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert_eq!(flags, vec![true, false]);
    // Spring DST: missing 02:00 resolves to 03:00; planning day switches then.
    sqlx::query("UPDATE scheduled_tasks SET weekdays=ARRAY[7]::smallint[]")
        .execute(&db.pool)
        .await
        .unwrap();
    support::generate_at(&db, "2026-03-29T00:59:00Z").await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scheduled_occurrences WHERE planning_date='2026-03-29'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    support::generate_at(&db, "2026-03-29T01:00:00Z").await;
    let expiry: String = sqlx::query_scalar("SELECT to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI') FROM scheduled_occurrences WHERE planning_date='2026-03-29'").fetch_one(&db.pool).await.unwrap();
    assert_eq!(expiry, "2026-03-30 00:00");
    let steps: sqlx::types::Json<Vec<Subtask>> = sqlx::query_scalar(
        "SELECT subtasks FROM scheduled_occurrences WHERE planning_date='2026-10-28'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(steps.0.iter().all(|s| !s.completed));
}

#[tokio::test]
async fn concurrent_generators_create_only_one_occurrence() {
    let db = TestDb::new().await;
    let svc = service(&db, db.user.id);
    svc.save(None, input()).await.unwrap();
    let (a, b) = tokio::join!(svc.daily(), svc.daily());
    assert_eq!(a.unwrap(), b.unwrap());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM scheduled_occurrences")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn scheduled_routes_require_auth_schedule_and_owned_occurrences() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;
    let db = TestDb::new().await;
    let sender = Arc::new(Sender::default());
    let app = db.app_with_sender(sender.clone());
    let call = |method: &str, path: &str, input: Value, user: i64| {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", support::authorization_for(user))
            .header("content-type", "application/json")
            .body(if input.is_null() {
                Body::empty()
            } else {
                Body::from(input.to_string())
            })
            .unwrap();
        let app = app.clone();
        async move {
            let response = app.oneshot(request).await.unwrap();
            let status = response.status().as_u16();
            let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/scheduled-tasks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);
    for value in [
        json!({"description":"Run"}),
        json!({"description":"Run","weekdays":null,"timezone":"UTC"}),
        json!({"description":"Run","weekdays":[1],"timezone":"UTC","user_id":1}),
        json!({"description":"Run","weekdays":[1],"timezone":"UTC","urgency":"soon"}),
    ] {
        assert_eq!(
            call("POST", "/scheduled-tasks", value, support::TELEGRAM_ID)
                .await
                .0,
            422
        );
    }
    assert_eq!(
        call(
            "POST",
            "/scheduled-tasks",
            json!({"description":"Run","weekdays":[],"timezone":"UTC"}),
            support::TELEGRAM_ID
        )
        .await
        .0,
        400
    );
    let (status, template) = call(
        "POST",
        "/scheduled-tasks",
        serde_json::to_value(input()).unwrap(),
        support::TELEGRAM_ID,
    )
    .await;
    assert_eq!(status, 201);
    assert!(template.get("category_id").is_none());
    assert!(template.get("urgency").is_none());
    assert_eq!(
        call(
            "GET",
            "/scheduled-tasks/daily",
            Value::Null,
            support::TELEGRAM_ID
        )
        .await
        .1,
        json!([])
    );
    accept_scheduled(&db).await;
    let (status, daily) = call(
        "GET",
        "/scheduled-tasks/daily",
        Value::Null,
        support::TELEGRAM_ID,
    )
    .await;
    assert_eq!(status, 200);
    assert!(daily[0].get("urgency").is_none());
    let id = daily[0]["id"].as_i64().unwrap();
    let select_path = format!("/scheduled-tasks/{}/daily", template["id"]);
    assert_eq!(call("POST", &select_path, Value::Null, 456).await.0, 404);
    assert_eq!(
        call("POST", &select_path, Value::Null, support::TELEGRAM_ID)
            .await
            .1,
        daily
    );
    let path = format!("/scheduled-tasks/daily/{id}/completion");
    assert_eq!(
        call("PUT", &path, json!({"completed":true}), 456).await.0,
        404
    );
    assert_eq!(
        call(
            "PUT",
            &path,
            json!({"completed":true,"extra":true}),
            support::TELEGRAM_ID
        )
        .await
        .0,
        422
    );
    let (status, daily) = call(
        "PUT",
        &path,
        json!({"completed":true}),
        support::TELEGRAM_ID,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(daily[0]["completed"], true);
    assert_eq!(
        call(
            "POST",
            "/daily-tasks/finalize",
            Value::Null,
            support::TELEGRAM_ID
        )
        .await
        .0,
        204
    );
    let summary = sender.0.lock().unwrap().join("\n");
    assert!(summary.contains("Sport exercises"));
    assert!(summary.contains('✅'));
    assert_eq!(
        call(
            "DELETE",
            &format!("/scheduled-tasks/{}", template["id"]),
            Value::Null,
            support::TELEGRAM_ID
        )
        .await
        .0,
        204
    );
    assert_eq!(
        call(
            "GET",
            "/scheduled-tasks/daily",
            Value::Null,
            support::TELEGRAM_ID
        )
        .await
        .1,
        json!([])
    );
}

#[tokio::test]
async fn removing_urgency_preserves_existing_schedules_and_daily_progress() {
    let db = TestDb::new_at_version(9).await;
    let id: i64 = sqlx::query_scalar("INSERT INTO scheduled_tasks(user_id, description, urgency, weekdays, timezone, subtasks) VALUES ($1, 'Exercises', 'soon', ARRAY[1,2,3,4,5,6,7]::smallint[], 'UTC', '[{\"description\":\"Run\",\"completed\":false}]') RETURNING id")
        .bind(db.user.id).fetch_one(&db.pool).await.unwrap();
    sqlx::query("SELECT generate_scheduled_occurrences($1, statement_timestamp())")
        .bind(db.user.id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE scheduled_occurrences SET completed=true, subtasks='[{\"description\":\"Run\",\"completed\":true}]'")
        .execute(&db.pool).await.unwrap();
    backend::db::MIGRATOR.run(&db.pool).await.unwrap();
    let svc = service(&db, db.user.id);
    let templates = svc.list().await.unwrap();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].id, id);
    assert_eq!(templates[0].details.description, "Exercises");
    let daily = svc.daily().await.unwrap();
    assert_eq!(daily.len(), 1);
    assert!(daily[0].completed);
    assert!(daily[0].subtasks[0].completed);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM information_schema.columns WHERE table_schema='public' AND table_name IN ('scheduled_tasks','scheduled_occurrences') AND column_name='urgency'")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn morning_opt_in_migration_preserves_existing_daily_progress_but_not_future_selection() {
    let db = TestDb::new_at_version(10).await;
    let svc = service(&db, db.user.id);
    // Seed the version-10 shape directly; the current DAO includes newer columns.
    sqlx::query("INSERT INTO scheduled_tasks(user_id, description, weekdays, timezone, subtasks) VALUES ($1, 'Sport exercises', ARRAY[1,2,3,4,5,6,7]::smallint[], 'Europe/Berlin', '[{\"description\":\"Warm up\",\"completed\":false},{\"description\":\"Run\",\"completed\":false}]')")
        .bind(db.user.id).execute(&db.pool).await.unwrap();
    sqlx::query("SELECT generate_scheduled_occurrences(NULL, statement_timestamp())")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE scheduled_occurrences SET completed=true, subtasks='[{\"description\":\"Run\",\"completed\":true}]'")
        .execute(&db.pool).await.unwrap();
    backend::db::MIGRATOR.run(&db.pool).await.unwrap();
    let current = svc.daily().await.unwrap();
    assert_eq!(current.len(), 1);
    assert!(current[0].completed);
    assert!(current[0].subtasks[0].completed);
    backend::dao::maintenance::generate_scheduled_at(
        &db.pool,
        Some(db.user.id),
        support::clock(&db).await + chrono::Duration::days(1),
    )
    .await
    .unwrap();
    let flags: Vec<bool> =
        sqlx::query_scalar("SELECT selected FROM scheduled_occurrences ORDER BY planning_date")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert_eq!(flags, vec![true, false]);
}
