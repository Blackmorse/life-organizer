mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use backend::domain::{Subtask, TaskUrgency};
use serde_json::{Value, json};
use support::TestDb;
use tower::ServiceExt;

#[derive(Default)]
struct RecordingSender {
    deliveries: std::sync::Mutex<Vec<(i64, Vec<String>)>>,
    failure: Option<backend::service::NotificationError>,
}

#[async_trait::async_trait]
impl backend::service::MessageSender for RecordingSender {
    async fn send(
        &self,
        recipient: i64,
        messages: &[String],
    ) -> Result<(), backend::service::NotificationError> {
        self.deliveries
            .lock()
            .unwrap()
            .push((recipient, messages.to_vec()));
        match self.failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    request_as(app, support::TELEGRAM_ID, method, path, body).await
}

async fn request_as(
    app: &Router,
    telegram_id: i64,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", support::authorization_for(telegram_id));
    let body = if let Some(body) = body {
        request = request.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

#[tokio::test]
async fn detailed_descriptions_round_trip_default_clear_and_follow_task_guards() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let sender = std::sync::Arc::new(RecordingSender::default());
    let app = db.app_with_sender(sender.clone());
    let input = json!({"category_id":category.id,"description":"Build","urgency":"soon"});
    let (status, task) = request(&app, "POST", "/tasks", Some(input.clone())).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(task["detailed_description"], "");
    let path = format!("/tasks/{}", task["id"].as_i64().unwrap());
    let notes = "First line\n<script>plain text</script> 📝";
    let mut detailed = input.clone();
    detailed["detailed_description"] = json!(notes);
    let (status, updated) = request(&app, "PUT", &path, Some(detailed.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["detailed_description"], notes);
    // A legacy edit/move omitting notes preserves them.
    assert_eq!(
        request(&app, "PUT", &path, Some(input.clone())).await.1["detailed_description"],
        notes
    );
    let steps = json!({"subtasks":[
        {"description":"Design","detailed_description":"Private step notes","completed":false},
        {"description":"Ship","completed":false}
    ]});
    let (_, updated) = request(&app, "PUT", &format!("{path}/subtasks"), Some(steps)).await;
    assert_eq!(
        updated["subtasks"][0]["detailed_description"],
        "Private step notes"
    );
    assert_eq!(updated["subtasks"][1]["detailed_description"], "");
    request(
        &app,
        "POST",
        "/daily-tasks",
        Some(json!({"task_id":task["id"],"timezone":"UTC"})),
    )
    .await;
    let (_, selected) = request(&db.app(), "GET", &path, None).await;
    let mut reordered = selected["subtasks"].as_array().unwrap().clone();
    reordered.reverse();
    let (_, updated) = request(
        &app,
        "PUT",
        &format!("{path}/subtasks"),
        Some(json!({"subtasks":reordered})),
    )
    .await;
    assert_eq!(
        updated["subtasks"][1]["detailed_description"],
        "Private step notes"
    );
    assert_eq!(updated["subtasks"][1]["id"], selected["subtasks"][0]["id"]);
    assert_eq!(
        request(&app, "POST", "/daily-tasks/finalize", None).await.0,
        StatusCode::NO_CONTENT
    );
    let messages = sender.deliveries.lock().unwrap()[0].1.join("\n");
    assert!(messages.contains("Design"));
    assert!(!messages.contains("Private step notes"));
    assert!(!messages.contains("First line"));
    assert_eq!(
        request_as(
            &app,
            support::TELEGRAM_ID + 1,
            "PUT",
            &path,
            Some(detailed.clone())
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    request(
        &app,
        "PUT",
        &format!("{path}/completion"),
        Some(json!({"completed":true})),
    )
    .await;
    assert_eq!(
        request(&app, "PUT", &path, Some(detailed.clone())).await.0,
        StatusCode::CONFLICT
    );
    let (_, archived) = request(&db.app(), "GET", &path, None).await;
    assert_eq!(archived["detailed_description"], notes);
    request(
        &app,
        "PUT",
        &format!("{path}/completion"),
        Some(json!({"completed":false})),
    )
    .await;
    detailed["detailed_description"] = json!("");
    assert_eq!(
        request(&app, "PUT", &path, Some(detailed)).await.1["detailed_description"],
        ""
    );

    // Category-free task APIs share the optional field and support explicit clearing.
    for (route, mut input) in [
        (
            "/scheduled-tasks",
            json!({"description":"Routine","weekdays":[1,2,3,4,5,6,7],"timezone":"UTC"}),
        ),
        (
            "/reminder-tasks",
            json!({"description":"Appointment","due_at":1804516200000_i64,"timezone":"UTC"}),
        ),
    ] {
        let (status, task) = request(&app, "POST", route, Some(input.clone())).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(task["detailed_description"], "");
        let path = format!("{route}/{}", task["id"].as_i64().unwrap());
        input["detailed_description"] = json!(notes);
        assert_eq!(
            request(&app, "PUT", &path, Some(input.clone())).await.1["detailed_description"],
            notes
        );
        assert_eq!(
            request(&db.app(), "GET", route, None).await.1[0]["detailed_description"],
            notes
        );
        input["detailed_description"] = json!("");
        assert_eq!(
            request(&app, "PUT", &path, Some(input)).await.1["detailed_description"],
            ""
        );
    }

    for payload in [
        json!({"category_id":category.id,"description":"Bad","detailed_description":42}),
        json!({"category_id":category.id,"description":"Bad","detailed_description":{}}),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tasks")
                    .header(
                        "authorization",
                        support::authorization_for(support::TELEGRAM_ID),
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

#[tokio::test]
async fn tomorrow_selection_is_independent_persistent_scoped_and_validated() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Plan", TaskUrgency::Soon)
        .await
        .unwrap();
    db.task_service()
        .replace_subtasks(task.id, &[Subtask::new("First"), Subtask::new("Second")])
        .await
        .unwrap();
    let app = db.app();
    for input in [
        json!({"task_id":task.id,"timezone":"Invalid/Zone"}),
        json!({"task_id":0,"timezone":"UTC"}),
    ] {
        assert_eq!(
            request(&app, "POST", "/tomorrow-tasks", Some(input))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            &app,
            "POST",
            "/tomorrow-tasks",
            Some(json!({"task_id":task.id,"timezone":"UTC","subtask_index":9}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request_as(
            &app,
            support::TELEGRAM_ID + 1,
            "POST",
            "/tomorrow-tasks",
            Some(json!({"task_id":task.id,"timezone":"UTC"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let input = json!({"task_id":task.id,"timezone":"Europe/Berlin"});
    let (status, selected) = request(&app, "POST", "/tomorrow-tasks", Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(selected.as_array().unwrap().len(), 2);
    assert_eq!(
        request(&app, "POST", "/tomorrow-tasks", Some(input))
            .await
            .1,
        selected
    );
    assert_eq!(
        request(&app, "GET", "/tomorrow-tasks", None).await.1,
        selected
    );
    assert_eq!(
        request(&app, "GET", "/daily-tasks", None).await.1,
        json!([])
    );
    assert_eq!(
        request_as(
            &app,
            support::TELEGRAM_ID + 1,
            "GET",
            "/tomorrow-tasks",
            None
        )
        .await
        .1,
        json!([])
    );
    let expiries: Vec<(String, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as("SELECT timezone,expires_at FROM tomorrow_tasks")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    for (zone, expiry) in expiries {
        assert_eq!(
            expiry,
            backend::planning::tomorrow_reset(
                backend::planning::zone(&zone).unwrap(),
                support::clock(&db).await
            )
        );
    }
    request(
        &app,
        "POST",
        "/daily-tasks",
        Some(json!({"task_id":task.id,"timezone":"UTC","subtask_index":0})),
    )
    .await;
    let removal = json!({"task_id":task.id,"subtask_id":selected[0]["subtask_id"]});
    request_as(
        &app,
        support::TELEGRAM_ID + 1,
        "DELETE",
        "/tomorrow-tasks",
        Some(removal.clone()),
    )
    .await;
    assert_eq!(
        request(&app, "GET", "/tomorrow-tasks", None).await.1,
        selected
    );
    assert_eq!(
        request(&app, "DELETE", "/tomorrow-tasks", Some(removal))
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        request(&app, "GET", "/daily-tasks", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Step replacement drops only removed identities, retaining reordered picks.
    let mut steps = db.task_service().get(task.id).await.unwrap().subtasks;
    steps.reverse();
    db.task_service()
        .replace_subtasks(task.id, &steps)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", "/tomorrow-tasks", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    db.task_service()
        .replace_subtasks(task.id, &[])
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", "/tomorrow-tasks", None).await.1,
        json!([])
    );
    request(
        &app,
        "POST",
        "/tomorrow-tasks",
        Some(json!({"task_id":task.id,"timezone":"UTC"})),
    )
    .await;
    db.task_service()
        .set_completed(task.id, true)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "POST",
            "/tomorrow-tasks",
            Some(json!({"task_id":task.id,"timezone":"UTC"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    request(
        &app,
        "DELETE",
        "/tomorrow-tasks",
        Some(json!({"task_id":task.id,"subtask_id":""})),
    )
    .await;
    assert_eq!(
        request(&app, "GET", "/tomorrow-tasks", None).await.1,
        json!([])
    );
}

#[tokio::test]
async fn finalizing_sends_only_current_owned_picks_to_verified_telegram_user() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work <&>").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Build", TaskUrgency::Soon)
        .await
        .unwrap();
    db.task_service()
        .replace_subtasks(task.id, &[Subtask::new("Design"), Subtask::new("Ship 🚀")])
        .await
        .unwrap();
    let expired = db
        .task_service()
        .create(category.id, "Expired", TaskUrgency::Someday)
        .await
        .unwrap();
    let sender = std::sync::Arc::new(RecordingSender::default());
    let app = db.app_with_sender(sender.clone());
    assert_eq!(
        request(&app, "POST", "/daily-tasks/finalize", None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(sender.deliveries.lock().unwrap().is_empty());
    for input in [
        json!({"task_id":task.id,"subtask_index":1,"timezone":"UTC"}),
        json!({"task_id":task.id,"timezone":"UTC"}),
        json!({"task_id":expired.id,"timezone":"UTC"}),
    ] {
        assert_eq!(
            request(&app, "POST", "/daily-tasks", Some(input)).await.0,
            StatusCode::OK
        );
    }
    sqlx::query("UPDATE daily_tasks SET expires_at = statement_timestamp() - interval '1 second' WHERE task_id = $1")
        .bind(expired.id).execute(&db.pool).await.unwrap();
    let mut saved = db.task_service().get(task.id).await.unwrap();
    saved.subtasks.swap(0, 1);
    saved.subtasks[0].description = "Release 🚀".into();
    saved.subtasks[0].completed = true;
    db.task_service()
        .replace_subtasks(task.id, &saved.subtasks)
        .await
        .unwrap();
    let picks = request(&app, "GET", "/daily-tasks", None).await.1;
    // Another user's empty list must not fall back to the first user's picks.
    assert_eq!(
        request_as(
            &app,
            support::TELEGRAM_ID + 1,
            "POST",
            "/daily-tasks/finalize",
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(sender.deliveries.lock().unwrap().is_empty());
    assert_eq!(
        request(&app, "POST", "/daily-tasks/finalize", None).await,
        (StatusCode::NO_CONTENT, Value::Null)
    );
    let deliveries = sender.deliveries.lock().unwrap().clone();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].0, support::TELEGRAM_ID);
    assert_eq!(
        deliveries[0].1.concat(),
        format!(
            "<b>Tasks for today are ready!</b>\n\n<b>📁 Work &lt;&amp;&gt;</b>\n<b>{indent}📋 Build</b>\n{indent}{indent}✅ Release 🚀\n{indent}{indent}🔹 Design\n\nProgress today: 1/2 tasks completed\n■■■■■□□□□□ 50%",
            indent = "\u{a0}".repeat(4)
        )
    );
    assert_eq!(request(&app, "GET", "/daily-tasks", None).await.1, picks);
    assert!(!db.task_service().get(task.id).await.unwrap().completed);
    // Finalize means send the current list, not lock it; explicit resends work.
    assert_eq!(
        request(&app, "POST", "/daily-tasks/finalize", None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(sender.deliveries.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn finalization_reports_delivery_errors_without_changing_picks() {
    use backend::service::NotificationError;
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Build", TaskUrgency::Soon)
        .await
        .unwrap();
    request(
        &db.app(),
        "POST",
        "/daily-tasks",
        Some(json!({"task_id":task.id,"timezone":"UTC"})),
    )
    .await;
    let picks = request(&db.app(), "GET", "/daily-tasks", None).await.1;
    for (failure, status, message) in [
        (
            NotificationError::ChatUnavailable,
            StatusCode::BAD_REQUEST,
            "/start",
        ),
        (
            NotificationError::DeliveryFailed,
            StatusCode::BAD_GATEWAY,
            "Check the bot chat before retrying",
        ),
    ] {
        let sender = std::sync::Arc::new(RecordingSender {
            failure: Some(failure),
            ..Default::default()
        });
        let app = db.app_with_sender(sender.clone());
        let (actual_status, error) = request(&app, "POST", "/daily-tasks/finalize", None).await;
        assert_eq!(actual_status, status);
        assert!(error["error"].as_str().unwrap().contains(message));
        assert_eq!(sender.deliveries.lock().unwrap().len(), 1);
        assert_eq!(request(&app, "GET", "/daily-tasks", None).await.1, picks);
    }
}

#[tokio::test]
async fn daily_picks_are_scoped_persistent_and_follow_subtask_identity() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Build", TaskUrgency::Soon)
        .await
        .unwrap();
    db.task_service()
        .replace_subtasks(task.id, &[Subtask::new("Same"), Subtask::new("Same")])
        .await
        .unwrap();
    let app = db.app();
    let select = json!({"task_id":task.id, "subtask_index":1, "timezone":"Europe/Berlin"});
    assert_eq!(
        request_as(
            &app,
            support::TELEGRAM_ID + 1,
            "POST",
            "/daily-tasks",
            Some(select.clone())
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/daily-tasks",
            Some(json!({"task_id":task.id,"timezone":"Bad/Zone"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/daily-tasks",
            Some(json!({"task_id":task.id,"subtask_index":9,"timezone":"UTC"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, picks) = request(&app, "POST", "/daily-tasks", Some(select.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(picks.as_array().unwrap().len(), 1);
    assert!(!picks[0]["subtask_id"].as_str().unwrap().is_empty());
    assert_eq!(
        request(&app, "POST", "/daily-tasks", Some(select))
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        request(&db.app(), "GET", "/daily-tasks", None).await.1,
        picks
    );
    assert_eq!(
        request_as(&app, support::TELEGRAM_ID + 1, "GET", "/daily-tasks", None)
            .await
            .1,
        json!([])
    );
    let mut saved = db.task_service().get(task.id).await.unwrap();
    assert!(saved.subtasks[0].id.is_none());
    assert_eq!(
        saved.subtasks[1].id.as_deref(),
        picks[0]["subtask_id"].as_str()
    );
    saved.subtasks.swap(0, 1);
    saved.subtasks[0].description = "Renamed".into();
    saved.subtasks[0].completed = true;
    let updated = db
        .task_service()
        .replace_subtasks(task.id, &saved.subtasks)
        .await
        .unwrap();
    assert!(!updated.completed);
    assert_eq!(request(&app, "GET", "/daily-tasks", None).await.1, picks);
    let mut forged = saved.subtasks.clone();
    forged[1].id = forged[0].id.clone();
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("/tasks/{}/subtasks", task.id),
            Some(json!({"subtasks":forged}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let whole = json!({"task_id":task.id,"timezone":"Europe/Berlin"});
    let all = request(&app, "POST", "/daily-tasks", Some(whole.clone()))
        .await
        .1;
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert!(
        all.as_array()
            .unwrap()
            .iter()
            .all(|pick| !pick["subtask_id"].as_str().unwrap().is_empty())
    );
    let saved_all = db.task_service().get(task.id).await.unwrap();
    assert!(saved_all.subtasks.iter().all(|step| step.id.is_some()));
    assert!(saved_all.subtasks[0].completed);
    assert!(!saved_all.subtasks[1].completed);
    assert_eq!(
        request(&app, "POST", "/daily-tasks", Some(whole.clone()))
            .await
            .1,
        all
    );
    let removal = json!({"task_id":task.id,"subtask_id":""});
    request_as(
        &app,
        support::TELEGRAM_ID + 1,
        "DELETE",
        "/daily-tasks",
        Some(removal.clone()),
    )
    .await;
    assert_eq!(
        request(&app, "GET", "/daily-tasks", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        request(&app, "DELETE", "/daily-tasks", Some(removal))
            .await
            .1,
        json!([])
    );
    // Individual selection/removal remains independent after bulk removal.
    request(
        &app,
        "POST",
        "/daily-tasks",
        Some(json!({"task_id":task.id,"subtask_index":0,"timezone":"UTC"})),
    )
    .await;
    db.task_service()
        .replace_subtasks(task.id, &[saved.subtasks[1].clone()])
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", "/daily-tasks", None).await.1,
        json!([])
    );
    request(&app, "POST", "/daily-tasks", Some(whole.clone())).await;
    sqlx::query("UPDATE daily_tasks SET expires_at = statement_timestamp() - interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", "/daily-tasks", None).await.1,
        json!([])
    );
    assert_eq!(
        db.task_service().get(task.id).await.unwrap().subtasks.len(),
        1
    );
    request(&app, "POST", "/daily-tasks", Some(whole.clone())).await;
    db.task_service()
        .set_completed(task.id, true)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "POST", "/daily-tasks", Some(whole)).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, "GET", "/daily-tasks", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    db.task_service()
        .set_completed(task.id, false)
        .await
        .unwrap();
    db.task_service().delete(task.id).await.unwrap();
    assert_eq!(
        request(&app, "GET", "/daily-tasks", None).await.1,
        json!([])
    );
}

#[tokio::test]
async fn daily_reset_uses_local_two_am_with_calendar_days_and_dst() {
    let db = TestDb::new().await;
    for (zone, now, expected) in [
        ("UTC", "2026-10-29 01:59:59+00", "2026-10-29 02:00:00+00"),
        ("UTC", "2026-10-29 02:00:00+00", "2026-10-30 02:00:00+00"),
        (
            "Asia/Kolkata",
            "2026-10-29 19:00:00+00",
            "2026-10-29 20:30:00+00",
        ),
        (
            "Europe/Berlin",
            "2026-03-28 12:00:00+00",
            "2026-03-29 01:00:00+00",
        ),
        (
            "Europe/Berlin",
            "2026-10-24 12:00:00+00",
            "2026-10-25 01:00:00+00",
        ),
        (
            "Europe/Berlin",
            "2026-10-25 00:30:00+00",
            "2026-10-25 01:00:00+00",
        ),
        (
            "Europe/Berlin",
            "2026-03-29 01:00:00+00",
            "2026-03-30 00:00:00+00",
        ),
    ] {
        assert_eq!(
            backend::planning::daily_reset(
                backend::planning::zone(zone).unwrap(),
                support::timestamp(&db, now).await
            ),
            support::timestamp(&db, expected).await,
            "{zone}: {now} -> {expected}"
        );
    }
}

#[tokio::test]
async fn subtask_completion_automatically_archives_parent_only_for_a_nonempty_finished_list() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Finish steps", TaskUrgency::Upcoming)
        .await
        .unwrap();
    let app = db.app();
    let path = format!("/tasks/{}", task.id);
    let steps_path = format!("{path}/subtasks");
    let completion_path = format!("{path}/completion");
    let steps = |first, second| {
        json!([
            {"description": "Design", "detailed_description": "", "completed": first},
            {"description": "Build", "detailed_description": "", "completed": second},
        ])
    };
    for (first, second, completed) in [
        (false, false, false),
        (true, false, false),
        (false, false, false),
        (true, true, true),
    ] {
        let input = steps(first, second);
        let (status, updated) =
            request(&app, "PUT", &steps_path, Some(json!({"subtasks": input}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["completed"], completed);
        assert_eq!(updated["subtasks"], input);
        assert_eq!(updated["urgency"], "upcoming");
        assert_eq!(updated["category_id"], category.id);
        assert_eq!(request(&db.app(), "GET", &path, None).await.1, updated);
        assert_eq!(
            request(&app, "GET", "/tasks", None).await.1,
            json!([updated])
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            &steps_path,
            Some(json!({"subtasks": steps(false, true)}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request_as(
            &app,
            support::TELEGRAM_ID + 1,
            "PUT",
            &steps_path,
            Some(json!({"subtasks": steps(false, false)}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, restored) = request(
        &app,
        "PUT",
        &completion_path,
        Some(json!({"completed": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["completed"], false);
    assert_eq!(restored["subtasks"], steps(true, true));
    // Restoring keeps the progress so an individual step can be reopened.
    assert_eq!(
        request(
            &app,
            "PUT",
            &steps_path,
            Some(json!({"subtasks": steps(false, true)}))
        )
        .await
        .1["completed"],
        false
    );
    // Text edits/reorder keep completion flags; new steps start unfinished.
    let reordered = json!([
        {"description": "Build edited", "detailed_description": "", "completed": true},
        {"description": "Design", "detailed_description": "", "completed": false},
        {"description": "Test", "detailed_description": "", "completed": false},
    ]);
    let (_, updated) = request(
        &app,
        "PUT",
        &steps_path,
        Some(json!({"subtasks": reordered})),
    )
    .await;
    assert_eq!(updated["subtasks"], reordered);
    assert_eq!(updated["completed"], false);
    // Deleting unfinished steps also completes a nonempty remaining finished list.
    let remaining =
        json!([{"description": "Build edited", "detailed_description": "", "completed": true}]);
    let (status, archived) = request(
        &app,
        "PUT",
        &steps_path,
        Some(json!({"subtasks": remaining})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(archived["completed"], true);
    assert_eq!(archived["subtasks"], remaining);
    request(
        &app,
        "PUT",
        &completion_path,
        Some(json!({"completed": false})),
    )
    .await;
    let (status, cleared) = request(&app, "PUT", &steps_path, Some(json!({"subtasks": []}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["completed"], false);
    assert_eq!(cleared["subtasks"], json!([]));
}

#[tokio::test]
async fn task_completion_archives_read_only_tasks_until_restored() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let destination = db.category_service().create("Home").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Build app", TaskUrgency::Soon)
        .await
        .unwrap();
    assert!(!task.completed);
    let app = db.app();
    let path = format!("/tasks/{}", task.id);
    let completion_path = format!("{path}/completion");
    // Completion does not require subtasks or depend on the planning horizon.
    for completed in [true, false] {
        let (status, result) = request(
            &app,
            "PUT",
            &completion_path,
            Some(json!({"completed": completed})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["completed"], completed);
    }
    let task = db
        .task_service()
        .replace_subtasks(task.id, &[Subtask::new("Design"), Subtask::new("Build")])
        .await
        .unwrap();
    let mut archived = serde_json::to_value(&task).unwrap();
    archived["completed"] = json!(true);
    // Both completion and restoration are idempotent, and only change the flag.
    for _ in 0..2 {
        assert_eq!(
            request(
                &app,
                "PUT",
                &completion_path,
                Some(json!({"completed": true}))
            )
            .await,
            (StatusCode::OK, archived.clone())
        );
    }
    assert_eq!(
        request(&db.app(), "GET", &path, None).await,
        (StatusCode::OK, archived.clone())
    );
    assert_eq!(
        request(&app, "GET", "/tasks", None).await.1,
        json!([archived.clone()])
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/tasks?category_id={}", category.id),
            None
        )
        .await
        .1,
        json!([archived.clone()])
    );

    for (method, target, body) in [
        (
            "PUT",
            path.clone(),
            Some(
                json!({"category_id": destination.id, "description": "Changed", "urgency": "someday", "completed": false}),
            ),
        ),
        (
            "PUT",
            format!("{path}/subtasks"),
            Some(json!({"subtasks": []})),
        ),
        ("DELETE", path.clone(), None),
    ] {
        assert_eq!(
            request(&app, method, &target, body).await,
            (
                StatusCode::CONFLICT,
                json!({"error": "archived tasks are read-only; restore the task first"})
            )
        );
        assert_eq!(request(&app, "GET", &path, None).await.1, archived);
    }
    // Categories with archived tasks cannot be removed either.
    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/categories/{}", category.id),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let other_user = support::TELEGRAM_ID + 1;
    for completed in [true, false] {
        assert_eq!(
            request_as(
                &app,
                other_user,
                "PUT",
                &completion_path,
                Some(json!({"completed": completed}))
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                &app,
                "PUT",
                "/tasks/999999/completion",
                Some(json!({"completed": completed}))
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            "/tasks/0/completion",
            Some(json!({"completed": true}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // Rejected completion payloads never alter the archived task.
    for body in [
        json!({}),
        json!({"completed": null}),
        json!({"completed": "false"}),
        json!({"completed": 0}),
        json!({"completed": false, "description": "Changed"}),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(&completion_path)
                    .header(
                        "authorization",
                        support::authorization_for(support::TELEGRAM_ID),
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(request(&app, "GET", &path, None).await.1, archived);
    }
    for _ in 0..2 {
        assert_eq!(
            request(
                &app,
                "PUT",
                &completion_path,
                Some(json!({"completed": false}))
            )
            .await,
            (StatusCode::OK, serde_json::to_value(&task).unwrap())
        );
    }
    let moved = db
        .task_service()
        .update(
            task.id,
            destination.id,
            "Reopened task",
            TaskUrgency::Upcoming,
        )
        .await
        .unwrap();
    assert!(!moved.completed);
    assert_eq!(moved.subtasks, task.subtasks);
    db.task_service()
        .replace_subtasks(task.id, &[])
        .await
        .unwrap();
    assert_eq!(
        request(&app, "DELETE", &path, None).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn ordered_subtasks_are_persisted_validated_and_user_scoped() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Build app", TaskUrgency::Soon)
        .await
        .unwrap();
    assert!(task.subtasks.is_empty());
    let app = db.app();
    let path = format!("/tasks/{}/subtasks", task.id);
    for body in [
        json!({}),
        json!({"subtasks": null}),
        json!({"subtasks": [1]}),
        json!({"subtasks": "step"}),
        json!({"subtasks": ["legacy text"]}),
        json!({"subtasks": [{"description": "Step"}]}),
        json!({"subtasks": [{"description": "Step", "completed": null}]}),
        json!({"subtasks": [{"description": "Step", "completed": "true"}]}),
        json!({"subtasks": [{"description": "Step", "completed": false, "extra": 1}]}),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(&path)
                    .header(
                        "authorization",
                        support::authorization_for(support::TELEGRAM_ID),
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
    for steps in [
        json!([
            Subtask::new("Design"),
            Subtask::new("Build"),
            Subtask::new("Test")
        ]),
        json!([Subtask::new("Test"), Subtask::new("Design updated")]),
    ] {
        let (status, updated) = request(&app, "PUT", &path, Some(json!({"subtasks": steps}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["subtasks"], steps);
        assert_eq!(updated["urgency"], "soon");
        assert_eq!(
            request(&app, "GET", &format!("/tasks/{}", task.id), None)
                .await
                .1,
            updated
        );
        assert_eq!(
            request(&app, "GET", "/tasks", None).await.1,
            json!([updated])
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            Some(json!({"subtasks": [Subtask::new("Valid"), Subtask::new(" \n\t")]}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/tasks/0/subtasks",
            Some(json!({"subtasks": []}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/tasks/999999/subtasks",
            Some(json!({"subtasks": []}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request_as(
            &app,
            support::TELEGRAM_ID + 1,
            "PUT",
            &path,
            Some(json!({"subtasks": [Subtask::new("Stolen")]}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let other = db.category_service().create("Home").await.unwrap();
    let moved = db
        .task_service()
        .update(task.id, other.id, "Moved", TaskUrgency::Upcoming)
        .await
        .unwrap();
    assert_eq!(
        moved.subtasks,
        [Subtask::new("Test"), Subtask::new("Design updated")]
    );
    // Backend validation rejects malformed lists atomically without DB routines.
    for invalid in [
        json!([Subtask::new("Valid"), Subtask::new("  ")]),
        json!([null]),
        json!(["step"]),
        json!({}),
        json!([[Subtask::new("Nested")]]),
        json!([{"description": "Step"}]),
        json!([{"description": "Step", "completed": "true"}]),
        json!([{"description": "Step", "completed": false, "extra": 1}]),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(&path)
                    .header(
                        "authorization",
                        support::authorization_for(support::TELEGRAM_ID),
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"subtasks":invalid}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
    assert_eq!(
        db.task_service().get(task.id).await.unwrap().subtasks,
        moved.subtasks
    );
    let (status, cleared) = request(&app, "PUT", &path, Some(json!({"subtasks": []}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["subtasks"], json!([]));
    db.task_service().delete(task.id).await.unwrap();
    assert_eq!(
        request(&app, "PUT", &path, Some(json!({"subtasks": []})))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn category_routes_support_crud() {
    let db = TestDb::new().await;
    let app = db.app();
    assert_eq!(
        request(&app, "GET", "/categories", None).await,
        (StatusCode::OK, json!([]))
    );
    let (status, category) = request(
        &app,
        "POST",
        "/categories",
        Some(json!({"description": "Work"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(category["id"].as_i64().unwrap() > 0);
    let path = format!("/categories/{}", category["id"]);
    assert_eq!(
        request(&app, "GET", &path, None).await,
        (StatusCode::OK, category.clone())
    );
    assert_eq!(
        request(&app, "GET", "/categories", None).await,
        (StatusCode::OK, json!([category]))
    );
    let (status, updated) = request(&app, "PUT", &path, Some(json!({"description": "Home"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["description"], "Home");
    assert_eq!(
        request(&app, "GET", &path, None).await,
        (StatusCode::OK, updated)
    );
    assert_eq!(
        request(&app, "DELETE", &path, None).await,
        (StatusCode::NO_CONTENT, Value::Null)
    );
    for method in ["GET", "PUT", "DELETE"] {
        let body = (method == "PUT").then(|| json!({"description": "Missing"}));
        assert_eq!(
            request(&app, method, &path, body).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn task_routes_support_crud_filtering_and_reassignment() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let other_category = db.category_service().create("Home").await.unwrap();
    let app = db.app();
    assert_eq!(
        request(&app, "GET", "/tasks", None).await,
        (StatusCode::OK, json!([]))
    );
    let (status, task) = request(
        &app,
        "POST",
        "/tasks",
        Some(json!({"category_id": category.id, "description": "Build backend"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(task["category_id"], category.id);
    assert_eq!(task["urgency"], "someday");
    assert!(task["id"].as_i64().unwrap() > 0);
    let path = format!("/tasks/{}", task["id"]);
    assert_eq!(
        request(&app, "GET", &path, None).await,
        (StatusCode::OK, task.clone())
    );
    assert_eq!(
        request(&app, "GET", "/tasks", None).await,
        (StatusCode::OK, json!([task]))
    );
    let filter = format!("/tasks?category_id={}", other_category.id);
    assert_eq!(
        request(&app, "GET", &filter, None).await,
        (StatusCode::OK, json!([]))
    );
    let (status, updated) = request(
        &app,
        "PUT",
        &path,
        Some(json!({"category_id": other_category.id, "description": "Clean desk", "urgency": "soon"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["category_id"], other_category.id);
    assert_eq!(updated["description"], "Clean desk");
    assert_eq!(updated["urgency"], "soon");
    assert_eq!(
        request(&app, "GET", &path, None).await,
        (StatusCode::OK, updated.clone())
    );
    assert_eq!(
        request(&app, "GET", &filter, None).await,
        (StatusCode::OK, json!([updated]))
    );
    assert_eq!(
        request(&app, "DELETE", &path, None).await,
        (StatusCode::NO_CONTENT, Value::Null)
    );
    for method in ["GET", "PUT", "DELETE"] {
        let body = (method == "PUT")
            .then(|| json!({"category_id": category.id, "description": "Missing"}));
        assert_eq!(
            request(&app, method, &path, body).await.0,
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn task_urgency_round_trips_and_rejects_invalid_updates() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let app = db.app();
    for urgency in ["someday", "upcoming", "soon"] {
        let (status, task) = request(
            &app,
            "POST",
            "/tasks",
            Some(json!({"category_id":category.id,"description":"Plan work","urgency":urgency})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(task["urgency"], urgency);
        let path = format!("/tasks/{}", task["id"]);
        assert_eq!(request(&app, "GET", &path, None).await.1, task);
        for invalid in [json!("critical"), json!(""), Value::Null, json!(1)] {
            let response = app.clone().oneshot(
                Request::builder().method("PUT").uri(&path)
                    .header("authorization", support::authorization_for(support::TELEGRAM_ID))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"category_id":category.id,"description":"Changed","urgency":invalid}).to_string()))
                    .unwrap()
            ).await.unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let expected = if invalid.is_string() {
                StatusCode::UNPROCESSABLE_ENTITY
            } else {
                StatusCode::BAD_REQUEST
            };
            assert_eq!(
                status,
                expected,
                "{invalid}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        assert_eq!(request(&app, "GET", &path, None).await.1, task);
    }
}

#[tokio::test]
async fn api_reports_validation_missing_categories_and_delete_conflicts() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Build backend", TaskUrgency::Someday)
        .await
        .unwrap();
    let app = db.app();
    for (method, path, body) in [
        (
            "POST",
            "/categories".to_owned(),
            json!({"description": "  "}),
        ),
        (
            "PUT",
            format!("/categories/{}", category.id),
            json!({"description": ""}),
        ),
        (
            "POST",
            "/tasks".to_owned(),
            json!({"category_id": category.id, "description": "\n"}),
        ),
        (
            "PUT",
            format!("/tasks/{}", task.id),
            json!({"category_id": category.id, "description": ""}),
        ),
        (
            "POST",
            "/tasks".to_owned(),
            json!({"category_id": 0, "description": "Invalid category"}),
        ),
    ] {
        let (status, error) = request(&app, method, &path, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(error["error"].is_string());
    }
    for path in ["/categories/0", "/tasks/-1", "/tasks?category_id=0"] {
        assert_eq!(
            request(&app, "GET", path, None).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for (method, path) in [
        ("POST", "/tasks".to_owned()),
        ("PUT", format!("/tasks/{}", task.id)),
    ] {
        let (status, error) = request(
            &app,
            method,
            &path,
            Some(json!({"category_id": 999, "description": "Orphan"})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error": "category not found"}));
    }
    let path = format!("/categories/{}", category.id);
    assert_eq!(
        request(&app, "DELETE", &path, None).await,
        (
            StatusCode::CONFLICT,
            json!({"error": "category still contains tasks"})
        )
    );
    assert_eq!(db.task_service().get(task.id).await.unwrap(), task);
}

#[tokio::test]
async fn api_rejects_malformed_payloads_and_hides_database_errors() {
    let db = TestDb::new().await;
    let app = db.app();
    // Axum's extractor rejections have their own response bodies.
    for (path, body, expected) in [
        ("/categories", "{", StatusCode::BAD_REQUEST),
        ("/categories", "{}", StatusCode::UNPROCESSABLE_ENTITY),
        (
            "/tasks",
            r#"{"description":"Missing category"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/tasks",
            r#"{"category_id":1,"description":"Invalid urgency","urgency":"critical"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/tasks",
            r#"{"category_id":1,"description":"Null urgency","urgency":null}"#,
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(
                        "authorization",
                        support::authorization_for(support::TELEGRAM_ID),
                    )
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let response_body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(
            status,
            expected,
            "{body}: {}",
            String::from_utf8_lossy(&response_body)
        );
    }
    db.pool.close().await;
    assert_eq!(
        request(&app, "GET", "/categories", None).await,
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "internal server error"})
        )
    );
}

#[tokio::test]
async fn every_route_requires_verified_telegram_identity() {
    let db = TestDb::new().await;
    let app = db.app();
    for (method, path) in [
        ("GET", "/me"),
        ("GET", "/daily-tasks"),
        ("POST", "/daily-tasks/finalize"),
        ("POST", "/daily-tasks"),
        ("DELETE", "/daily-tasks"),
        ("GET", "/tomorrow-tasks"),
        ("POST", "/tomorrow-tasks"),
        ("DELETE", "/tomorrow-tasks"),
        ("GET", "/categories"),
        ("POST", "/categories"),
        ("GET", "/categories/1"),
        ("PUT", "/categories/1"),
        ("DELETE", "/categories/1"),
        ("GET", "/tasks"),
        ("POST", "/tasks"),
        ("GET", "/tasks/1"),
        ("PUT", "/tasks/1"),
        ("PUT", "/tasks/1/subtasks"),
        ("PUT", "/tasks/1/completion"),
        ("DELETE", "/tasks/1"),
    ] {
        for header in [
            None,
            Some("Bearer 7000000001"),
            Some("tma user=%7B%22id%22%3A7000000001%7D&hash=00"),
        ] {
            let mut builder = Request::builder().method(method).uri(path);
            if let Some(header) = header {
                builder = builder.header("authorization", header);
            }
            let response = app
                .clone()
                .oneshot(builder.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {path}"
            );
        }
    }
    let valid = support::authorization_for(support::TELEGRAM_ID);
    for value in [
        valid.replace("7000000001", "7000000002"),
        format!("{valid}&auth_date=0"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header("authorization", value)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header("authorization", &valid)
                .header("authorization", &valid)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "invalid credentials must not register users");
    assert!(db.category_service().list().await.unwrap().is_empty());
}

#[tokio::test]
async fn users_are_registered_once_and_all_crud_is_isolated() {
    let db = TestDb::new().await;
    let app = db.app();
    let alice = support::TELEGRAM_ID;
    let bob = alice + 1;
    let (status, bob_user) = request_as(&app, bob, "GET", "/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bob_user["telegram_id"], bob);
    assert_ne!(bob_user["id"], db.user.id);
    assert_eq!(request_as(&app, bob, "GET", "/me", None).await.1, bob_user);

    let (status, category) = request_as(
        &app,
        alice,
        "POST",
        "/categories",
        Some(json!({"description":"Private work", "user_id":bob_user["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        category["user_id"], db.user.id,
        "client cannot choose owner"
    );
    let category_id = category["id"].as_i64().unwrap();
    let (_, bob_category) = request_as(
        &app,
        bob,
        "POST",
        "/categories",
        Some(json!({"description":"Bob work"})),
    )
    .await;
    let bob_category_id = bob_category["id"].as_i64().unwrap();
    let (status, task) = request_as(
        &app,
        alice,
        "POST",
        "/tasks",
        Some(json!({"category_id":category_id,"description":"Private task"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let task_id = task["id"].as_i64().unwrap();
    let (_, bob_task) = request_as(
        &app,
        bob,
        "POST",
        "/tasks",
        Some(json!({"category_id":bob_category_id,"description":"Bob task"})),
    )
    .await;

    assert_eq!(
        request_as(&app, bob, "GET", "/categories", None).await.1,
        json!([bob_category])
    );
    assert_eq!(
        request_as(&app, bob, "GET", "/tasks", None).await.1,
        json!([bob_task])
    );
    assert_eq!(
        request_as(
            &app,
            bob,
            "GET",
            &format!("/tasks?category_id={category_id}"),
            None
        )
        .await
        .1,
        json!([])
    );
    for (method, path, body) in [
        ("GET", format!("/categories/{category_id}"), None),
        (
            "PUT",
            format!("/categories/{category_id}"),
            Some(json!({"description":"Stolen", "user_id":bob_user["id"]})),
        ),
        ("DELETE", format!("/categories/{category_id}"), None),
        ("GET", format!("/tasks/{task_id}"), None),
        (
            "PUT",
            format!("/tasks/{task_id}"),
            Some(json!({"category_id":bob_category_id,"description":"Stolen"})),
        ),
        ("DELETE", format!("/tasks/{task_id}"), None),
        (
            "POST",
            "/tasks".to_owned(),
            Some(json!({"category_id":category_id,"description":"Intruder"})),
        ),
    ] {
        assert_eq!(
            request_as(&app, bob, method, &path, body).await.0,
            StatusCode::NOT_FOUND,
            "{method} {path}"
        );
    }
    // An owner cannot move their own task into somebody else's category either.
    assert_eq!(
        request_as(
            &app,
            alice,
            "PUT",
            &format!("/tasks/{task_id}"),
            Some(json!({"category_id":bob_category_id,"description":"Cross-user move"}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request_as(&app, alice, "GET", &format!("/tasks/{task_id}"), None)
            .await
            .1,
        task
    );
    assert_eq!(
        request_as(
            &app,
            alice,
            "GET",
            &format!("/categories/{category_id}"),
            None
        )
        .await
        .1,
        category
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header("authorization", support::authorization_for(alice))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
}
