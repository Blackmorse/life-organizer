mod support;

use backend::{dao::TaskDao, db::MIGRATOR};
use serde_json::json;
use support::TestDb;

#[tokio::test]
async fn upgrade_defaults_existing_work_and_validates_optional_step_notes() {
    let db = TestDb::new_at_version(16).await;
    let category = db.category_service().create("Work").await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO tasks(category_id, description, subtasks) VALUES ($1, 'Legacy task', '[{\"id\":\"17\",\"description\":\"Legacy step\",\"completed\":true}]') RETURNING id")
        .bind(category.id).fetch_one(&db.pool).await.unwrap();
    let schedule_id: i64 = sqlx::query_scalar("INSERT INTO scheduled_tasks(user_id, description, weekdays, timezone) VALUES ($1, 'Legacy routine', ARRAY[1,2,3,4,5,6,7]::smallint[], 'UTC') RETURNING id")
        .bind(db.user.id).fetch_one(&db.pool).await.unwrap();
    sqlx::query("SELECT generate_scheduled_occurrences($1, statement_timestamp())")
        .bind(db.user.id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO reminder_tasks(user_id, description, due_at, timezone) VALUES ($1, 'Legacy reminder', statement_timestamp(), 'UTC')")
        .bind(db.user.id).execute(&db.pool).await.unwrap();
    MIGRATOR.run(&db.pool).await.unwrap();
    let task = db.tasks().get(id).await.unwrap().unwrap();
    assert_eq!(task.detailed_description, "");
    assert_eq!(task.subtasks[0].detailed_description, "");
    assert_eq!(task.subtasks[0].id.as_deref(), Some("17"));
    assert!(task.subtasks[0].completed);
    for table in ["scheduled_tasks", "scheduled_occurrences", "reminder_tasks"] {
        let values: Vec<String> =
            sqlx::query_scalar(&format!("SELECT detailed_description FROM {table}"))
                .fetch_all(&db.pool)
                .await
                .unwrap();
        assert_eq!(values, [""]);
    }
    for (notes, valid) in [
        (json!(""), true),
        (json!("line one\nline two"), true),
        (json!(null), false),
        (json!(42), false),
        (json!({}), false),
    ] {
        let steps = json!([{"description":"Step","completed":false,"detailed_description":notes}]);
        let parsed = serde_json::from_value::<Vec<backend::domain::Subtask>>(steps);
        assert_eq!(parsed.is_ok(), valid);
        if let Ok(steps) = parsed {
            db.task_service()
                .replace_subtasks(id, &steps)
                .await
                .unwrap();
        }
    }
    // Future snapshots copy notes; migration never rewrites today's snapshot.
    sqlx::query("UPDATE scheduled_tasks SET detailed_description='Future notes' WHERE id=$1")
        .bind(schedule_id)
        .execute(&db.pool)
        .await
        .unwrap();
    backend::dao::maintenance::generate_scheduled_at(
        &db.pool,
        Some(db.user.id),
        support::clock(&db).await + chrono::Duration::days(1),
    )
    .await
    .unwrap();
    let notes: Vec<String> = sqlx::query_scalar(
        "SELECT detailed_description FROM scheduled_occurrences ORDER BY planning_date",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(notes, ["", "Future notes"]);
}
