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
async fn moving_selected_work_preserves_progress_and_hands_off_only_on_target_date() {
    use backend::{
        dao::{
            maintenance,
            morning::{Decision, MorningStore},
        },
        planning,
    };
    use chrono::{Duration, Utc};
    let db = TestDb::new().await;
    let category = db.category_service().create("Work").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Release", TaskUrgency::Soon)
        .await
        .unwrap();
    let mut done = Subtask::new("Built");
    done.completed = true;
    db.task_service()
        .replace_subtasks(
            task.id,
            &[done, Subtask::new("Deploy"), Subtask::new("Unselected")],
        )
        .await
        .unwrap();
    let daily = PgDailyDao::new(db.pool.clone(), db.user.id);
    daily
        .select(task.id, Some(0), "Europe/Berlin")
        .await
        .unwrap();
    daily
        .select(task.id, Some(1), "Europe/Berlin")
        .await
        .unwrap();
    let original = db.task_service().get(task.id).await.unwrap();
    let dao = PgDailyWorkDao::new(db.pool.clone(), db.user.id);
    let item = find(&dao.list().await.unwrap(), &format!("task:{}", task.id));
    let day = (Utc::now() + Duration::days(14)).date_naive();
    let result = dao
        .move_to_date(&item, &day.to_string(), None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.timezone, "Europe/Berlin");
    assert!(daily.list().await.unwrap().is_empty());
    assert_eq!(db.task_service().get(task.id).await.unwrap(), original);
    assert!(
        backend::dao::BacklogDao::list(&backend::dao::PgBacklogDao::new(
            db.pool.clone(),
            db.user.id
        ))
        .await
        .unwrap()
        .is_empty()
    );
    assert!(
        PgDailyDao::tomorrow(db.pool.clone(), db.user.id)
            .list()
            .await
            .unwrap()
            .is_empty()
    );
    let zone = planning::zone(&result.timezone).unwrap();
    let reset = planning::at(zone, day, 2);
    let plans: Vec<(String, chrono::DateTime<Utc>)> =
        sqlx::query_as("SELECT subtask_id,expires_at FROM tomorrow_tasks WHERE task_id=$1")
            .bind(task.id)
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert_eq!(
        plans,
        vec![(original.subtasks[1].id.clone().unwrap(), reset)]
    );
    let mut tx = db.pool.begin().await.unwrap();
    maintenance::expire_picks(
        &mut tx,
        "tomorrow_tasks",
        Some(db.user.id),
        None,
        reset - Duration::seconds(1),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offer_items")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut tx = db.pool.begin().await.unwrap();
    maintenance::expire_picks(&mut tx, "tomorrow_tasks", Some(db.user.id), None, reset)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (offer, due, until): (i64, chrono::DateTime<Utc>, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT id,due_at,valid_until FROM morning_offers WHERE reset_at=$1")
            .bind(reset)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(due, planning::at(zone, day, 8));
    assert_eq!(until, planning::at(zone, day.succ_opt().unwrap(), 2));
    let store = MorningStore::new(db.pool.clone());
    assert_eq!(
        store
            .decide(offer, support::TELEGRAM_ID, true)
            .await
            .unwrap(),
        Decision::Unavailable
    );
    // Bring delivery forward to exercise existing acceptance with the preserved stable ID.
    sqlx::query(
        "UPDATE morning_offers SET due_at=statement_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(offer)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        store
            .decide(offer, support::TELEGRAM_ID, true)
            .await
            .unwrap(),
        Decision::Accepted(1)
    );
    assert_eq!(
        daily.list().await.unwrap()[0].subtask_id,
        original.subtasks[1].id.clone().unwrap()
    );
}

#[tokio::test]
async fn moving_rejects_stale_foreign_invalid_completed_and_expired_work_without_losing_picks() {
    let db = TestDb::new().await;
    let category = db.category_service().create("Home").await.unwrap();
    let task = db
        .task_service()
        .create(category.id, "Groceries", TaskUrgency::Soon)
        .await
        .unwrap();
    let daily = PgDailyDao::new(db.pool.clone(), db.user.id);
    daily.select(task.id, None, "UTC").await.unwrap();
    let dao = PgDailyWorkDao::new(db.pool.clone(), db.user.id);
    let item = dao.list().await.unwrap().remove(0);
    for date in ["2020-01-01", "2027-02-30", "next Friday", "2030-1-1"] {
        assert!(dao.move_to_date(&item, date, None).await.is_err());
    }
    assert!(
        dao.move_to_date(&item, "tomorrow", Some("bad-zone"))
            .await
            .is_err()
    );
    let foreign = PgDailyWorkDao::new(db.pool.clone(), db.user.id + 1);
    assert!(
        foreign
            .move_to_date(&item, "tomorrow", None)
            .await
            .unwrap()
            .is_none()
    );
    let mut stale = item.clone();
    stale.task = "Old name".into();
    assert!(
        dao.move_to_date(&stale, "tomorrow", None)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(daily.list().await.unwrap().len(), 1);
    db.task_service()
        .set_completed(task.id, true)
        .await
        .unwrap();
    assert!(
        dao.move_to_date(&item, "tomorrow", None)
            .await
            .unwrap()
            .is_none()
    );
    db.task_service()
        .set_completed(task.id, false)
        .await
        .unwrap();
    sqlx::query("UPDATE daily_tasks SET expires_at=statement_timestamp()-interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        dao.move_to_date(&item, "tomorrow", None)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn reminder_move_preserves_local_time_across_dst_and_invalidates_old_offer() {
    use chrono::{TimeZone, Utc};
    let db = TestDb::new().await;
    let id: i64 = sqlx::query_scalar("INSERT INTO reminder_tasks(user_id,description,due_at,timezone,selected_until) VALUES($1,'Dentist','2030-03-20 08:45:00Z','Europe/Berlin',statement_timestamp()+interval '1 hour') RETURNING id")
        .bind(db.user.id).fetch_one(&db.pool).await.unwrap();
    let dao = PgDailyWorkDao::new(db.pool.clone(), db.user.id);
    let item = find(&dao.list().await.unwrap(), &format!("reminder:{id}"));
    let offer: i64 = sqlx::query_scalar("INSERT INTO morning_offers(user_id,timezone,reset_at,due_at,valid_until,state) VALUES($1,'Europe/Berlin',statement_timestamp()-interval '6 hours',statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','sent') RETURNING id")
        .bind(db.user.id).fetch_one(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO morning_offer_reminder_items(offer_id,reminder_id,due_at,timezone,kind) SELECT $1,id,due_at,timezone,'today' FROM reminder_tasks WHERE id=$2")
        .bind(offer).bind(id).execute(&db.pool).await.unwrap();
    let moved = dao
        .move_to_date(&item, "2030-04-02", Some("America/New_York"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(moved.timezone, "Europe/Berlin");
    let (due, selected, completed): (chrono::DateTime<Utc>, Option<chrono::DateTime<Utc>>, bool) =
        sqlx::query_as("SELECT due_at,selected_until,completed FROM reminder_tasks WHERE id=$1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(due, Utc.with_ymd_and_hms(2030, 4, 2, 7, 45, 0).unwrap());
    assert!(selected.is_none());
    assert!(!completed);
    assert_eq!(
        backend::dao::morning::MorningStore::new(db.pool.clone())
            .decide(offer, support::TELEGRAM_ID, true)
            .await
            .unwrap(),
        backend::dao::morning::Decision::Accepted(0)
    );
    backend::dao::maintenance::prepare_reminders_at(
        &db.pool,
        Utc.with_ymd_and_hms(2030, 4, 2, 6, 0, 0).unwrap(),
    )
    .await
    .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM morning_offer_reminder_items WHERE reminder_id=$1 AND due_at=$2 AND kind='today'").bind(id).bind(due).fetch_one(&db.pool).await.unwrap();
    assert_eq!(count, 1);
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
