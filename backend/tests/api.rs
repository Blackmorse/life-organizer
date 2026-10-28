mod support;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use support::TestDb;
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(path);
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
        Some(json!({"category_id": other_category.id, "description": "Clean desk"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["category_id"], other_category.id);
    assert_eq!(updated["description"], "Clean desk");
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
async fn api_reports_validation_missing_categories_and_delete_conflicts() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Build backend")
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
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
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
