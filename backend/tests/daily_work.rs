//! DAO/service integration tests for LLM-facing daily actions; no external LLM.
mod support;

use backend::{
    dao::{
        DailyDao, PgDailyDao, PgUserDao, TaskDao,
        daily_work::{DailyWork, DailyWorkDao, PgDailyWorkDao},
    },
    domain::{Subtask, TaskUrgency},
    service::DailyWorkService,
};
use std::sync::Arc;
use support::TestDb;

fn find(items: &[DailyWork], id: &str) -> DailyWork {
    items.iter().find(|item| item.id == id).unwrap().clone()
}

#[tokio::test]
async fn selected_steps_keep_stable_ids_and_auto_archive_only_when_all_steps_done() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Release", TaskUrgency::Soon)
        .await
        .unwrap();
    db.task_service()
        .replace_subtasks(task.id, &[Subtask::new("Build"), Subtask::new("Deploy")])
        .await
        .unwrap();
    let daily = PgDailyDao::new(db.pool.clone(), db.user.id);
    daily.select(task.id, None, "UTC").await.unwrap();
    let task = db.task_service().get(task.id).await.unwrap();
    let service = DailyWorkService::new(Arc::new(PgDailyWorkDao::new(db.pool.clone(), db.user.id)));
    let items = service.list().await.unwrap();
    assert_eq!(items.len(), 3); // Parent context plus both selected steps.
    let build = find(
        &items,
        &format!(
            "subtask:{}:{}",
            task.id,
            task.subtasks[0].id.as_ref().unwrap()
        ),
    );
    let deploy = find(
        &items,
        &format!(
            "subtask:{}:{}",
            task.id,
            task.subtasks[1].id.as_ref().unwrap()
        ),
    );
    // The voice request can race a reorder; stable identity must win over index.
    let mut reordered = task.subtasks.clone();
    reordered.reverse();
    db.task_service()
        .replace_subtasks(task.id, &reordered)
        .await
        .unwrap();
    service.complete(&build).await.unwrap();
    let updated = db.task_service().get(task.id).await.unwrap();
    assert!(!updated.completed);
    assert!(!updated.subtasks[0].completed);
    assert!(updated.subtasks[1].completed);
    service.complete(&build).await.unwrap(); // Idempotent, never toggle.
    service.complete(&deploy).await.unwrap();
    let updated = db.task_service().get(task.id).await.unwrap();
    assert!(updated.completed);
    assert!(updated.subtasks.iter().all(|step| step.completed));
    assert_eq!(daily.list().await.unwrap().len(), 2); // Picks stay visible.
    assert!(
        service
            .list()
            .await
            .unwrap()
            .iter()
            .all(|item| item.completed)
    );
}

#[tokio::test]
async fn rejects_other_owners_unselected_removed_expired_and_changed_work() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Release", TaskUrgency::Soon)
        .await
        .unwrap();
    let daily = PgDailyDao::new(db.pool.clone(), db.user.id);
    let dao = PgDailyWorkDao::new(db.pool.clone(), db.user.id);
    let guessed = DailyWork {
        id: format!("task:{}", task.id),
        task: task.description.clone(),
        category: "Work".into(),
        subtask: None,
        completed: false,
    };
    assert!(dao.list().await.unwrap().is_empty());
    assert!(!dao.complete(&guessed).await.unwrap());
    daily.select(task.id, None, "UTC").await.unwrap();
    let item = find(&dao.list().await.unwrap(), &guessed.id);
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(12345)
        .await
        .unwrap();
    let foreign = PgDailyWorkDao::new(db.pool.clone(), other.id);
    assert!(foreign.list().await.unwrap().is_empty());
    assert!(!foreign.complete(&item).await.unwrap());
    daily.remove(task.id, "").await.unwrap();
    assert!(!dao.complete(&item).await.unwrap());
    daily.select(task.id, None, "UTC").await.unwrap();
    sqlx::query("UPDATE daily_tasks SET expires_at=statement_timestamp()-interval '1 second' WHERE task_id=$1")
        .bind(task.id).execute(&db.pool).await.unwrap();
    assert!(dao.list().await.unwrap().is_empty());
    assert!(!dao.complete(&item).await.unwrap());
    daily.select(task.id, None, "UTC").await.unwrap();
    db.task_service()
        .update(task.id, category.id, "Different work", TaskUrgency::Soon)
        .await
        .unwrap();
    assert!(!dao.complete(&item).await.unwrap());
    assert!(!db.tasks().get(task.id).await.unwrap().unwrap().completed);
    let current = dao.list().await.unwrap().remove(0);
    assert!(dao.complete(&current).await.unwrap());
    assert!(db.tasks().get(task.id).await.unwrap().unwrap().completed);
}

#[tokio::test]
async fn lists_and_completes_only_selected_scheduled_occurrences_and_reminders() {
    let db = TestDb::new().await;
    let template: i64 = sqlx::query_scalar("INSERT INTO scheduled_tasks(user_id, description, weekdays, timezone) VALUES ($1,'Exercise',ARRAY[1,2,3,4,5,6,7]::smallint[],'UTC') RETURNING id")
        .bind(db.user.id).fetch_one(&db.pool).await.unwrap();
    backend::dao::generate_scheduled(&db.pool, Some(db.user.id))
        .await
        .unwrap();
    let occurrence: i64 = sqlx::query_scalar("UPDATE scheduled_occurrences SET subtasks='[{\"description\":\"Warm up\",\"completed\":false},{\"description\":\"Run\",\"completed\":false}]' WHERE scheduled_task_id=$1 RETURNING id")
        .bind(template).fetch_one(&db.pool).await.unwrap();
    let reminder: i64 = sqlx::query_scalar("INSERT INTO reminder_tasks(user_id, description, due_at, timezone) VALUES ($1,'Dentist',statement_timestamp(),'UTC') RETURNING id")
        .bind(db.user.id).fetch_one(&db.pool).await.unwrap();
    let dao = PgDailyWorkDao::new(db.pool.clone(), db.user.id);
    assert!(dao.list().await.unwrap().is_empty());
    sqlx::query("UPDATE scheduled_occurrences SET selected=true WHERE id=$1")
        .bind(occurrence)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE reminder_tasks SET selected_until=statement_timestamp()+interval '1 hour' WHERE id=$1").bind(reminder).execute(&db.pool).await.unwrap();
    let items = dao.list().await.unwrap();
    assert_eq!(items.len(), 4);
    let step = find(&items, &format!("scheduled_step:{occurrence}:0"));
    assert!(dao.complete(&step).await.unwrap());
    let parent = find(
        &dao.list().await.unwrap(),
        &format!("scheduled:{occurrence}"),
    );
    assert!(!parent.completed);
    assert!(dao.complete(&parent).await.unwrap());
    let original: bool =
        sqlx::query_scalar("SELECT subtasks='[]'::jsonb FROM scheduled_tasks WHERE id=$1")
            .bind(template)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(original); // Template stays untouched.
    let reminder_item = find(&items, &format!("reminder:{reminder}"));
    assert!(dao.complete(&reminder_item).await.unwrap());
    assert!(dao.complete(&reminder_item).await.unwrap());
    let other = PgUserDao::new(db.pool.clone())
        .get_or_create(12345)
        .await
        .unwrap();
    let foreign = PgDailyWorkDao::new(db.pool.clone(), other.id);
    assert!(foreign.list().await.unwrap().is_empty());
    assert!(!foreign.complete(&parent).await.unwrap());
    assert!(!foreign.complete(&reminder_item).await.unwrap());
    sqlx::query("UPDATE scheduled_occurrences SET hidden=true WHERE id=$1")
        .bind(occurrence)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE reminder_tasks SET selected_until=NULL WHERE id=$1")
        .bind(reminder)
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(dao.list().await.unwrap().is_empty());
    assert!(!dao.complete(&parent).await.unwrap());
    assert!(!dao.complete(&reminder_item).await.unwrap());
}
