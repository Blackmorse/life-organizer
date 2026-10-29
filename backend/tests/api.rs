mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use backend::domain::TaskUrgency;
use serde_json::{Value, json};
use support::TestDb;
use tower::ServiceExt;

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
        ("GET", "/categories"),
        ("POST", "/categories"),
        ("GET", "/categories/1"),
        ("PUT", "/categories/1"),
        ("DELETE", "/categories/1"),
        ("GET", "/tasks"),
        ("POST", "/tasks"),
        ("GET", "/tasks/1"),
        ("PUT", "/tasks/1"),
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
