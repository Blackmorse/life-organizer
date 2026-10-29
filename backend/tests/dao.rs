mod support;

use backend::{
    dao::{CategoryDao, DaoError, TaskDao},
    db::MIGRATOR,
    domain::TaskUrgency,
};
use support::TestDb;

#[tokio::test]
async fn category_crud_and_missing_rows() {
    let db = TestDb::new().await;
    let dao = db.categories();
    assert!(dao.list().await.unwrap().is_empty());
    let first = dao.create("Work").await.unwrap();
    let second = dao.create("Work").await.unwrap();
    assert!(first.id > 0);
    assert_ne!(first.id, second.id, "descriptions are not natural keys");
    assert_eq!(dao.get(first.id).await.unwrap(), Some(first.clone()));
    assert_eq!(
        dao.list().await.unwrap(),
        vec![first.clone(), second.clone()]
    );
    let updated = dao.update(first.id, "Personal").await.unwrap().unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.description, "Personal");
    assert_eq!(dao.get(first.id).await.unwrap(), Some(updated));
    assert!(dao.delete(first.id).await.unwrap());
    assert!(dao.get(first.id).await.unwrap().is_none());
    assert!(dao.update(first.id, "Missing").await.unwrap().is_none());
    assert!(!dao.delete(first.id).await.unwrap());
    assert_eq!(dao.list().await.unwrap(), vec![second]);
}

#[tokio::test]
async fn task_crud_filtering_and_reassignment() {
    let db = TestDb::new().await;
    let categories = db.categories();
    let dao = db.tasks();
    let first = categories.create("Work").await.unwrap();
    let second = categories.create("Home").await.unwrap();
    assert!(dao.list(None).await.unwrap().is_empty());
    let task = dao
        .create(first.id, "Build backend", TaskUrgency::Upcoming)
        .await
        .unwrap();
    let other = dao
        .create(first.id, "Write docs", TaskUrgency::Someday)
        .await
        .unwrap();
    assert!(task.id > 0);
    assert_ne!(task.id, other.id);
    assert_eq!(task.urgency, TaskUrgency::Upcoming);
    assert_eq!(dao.get(task.id).await.unwrap(), Some(task.clone()));
    assert_eq!(
        dao.list(None).await.unwrap(),
        vec![task.clone(), other.clone()]
    );
    let updated = dao
        .update(task.id, second.id, "Clean desk", TaskUrgency::Soon)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.id, task.id);
    assert_eq!(updated.category_id, second.id);
    assert_eq!(updated.description, "Clean desk");
    assert_eq!(updated.urgency, TaskUrgency::Soon);
    assert_eq!(dao.get(task.id).await.unwrap(), Some(updated.clone()));
    assert_eq!(dao.list(Some(first.id)).await.unwrap(), vec![other]);
    assert_eq!(dao.list(Some(second.id)).await.unwrap(), vec![updated]);
    assert!(dao.delete(task.id).await.unwrap());
    assert!(dao.get(task.id).await.unwrap().is_none());
    assert!(
        dao.update(task.id, first.id, "Missing", TaskUrgency::Someday)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!dao.delete(task.id).await.unwrap());
}

#[tokio::test]
async fn foreign_keys_prevent_orphans_and_accidental_cascade_deletion() {
    let db = TestDb::new().await;
    let categories = db.categories();
    let tasks = db.tasks();
    assert!(matches!(
        tasks.create(999, "Orphan", TaskUrgency::Someday).await,
        Err(DaoError::CategoryMissing)
    ));
    let category = categories.create("Work").await.unwrap();
    let task = tasks
        .create(category.id, "Build backend", TaskUrgency::Soon)
        .await
        .unwrap();
    assert!(matches!(
        tasks
            .update(task.id, 999, "Orphan", TaskUrgency::Upcoming)
            .await,
        Err(DaoError::CategoryMissing)
    ));
    assert_eq!(tasks.get(task.id).await.unwrap(), Some(task.clone()));
    assert!(matches!(
        categories.delete(category.id).await,
        Err(DaoError::CategoryNotEmpty)
    ));
    assert_eq!(tasks.get(task.id).await.unwrap(), Some(task.clone()));
    assert!(categories.get(category.id).await.unwrap().is_some());
    tasks.delete(task.id).await.unwrap();
    assert!(categories.delete(category.id).await.unwrap());
}

#[tokio::test]
async fn schema_enforces_required_fields_and_migrations_are_repeatable() {
    let db = TestDb::new().await;
    let category = db.categories().create("Work").await.unwrap();
    for description in ["", "  \n\t"] {
        let error = sqlx::query("INSERT INTO categories (description, user_id) VALUES ($1, $2)")
            .bind(description)
            .bind(db.user.id)
            .execute(&db.pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        let error = sqlx::query("INSERT INTO tasks (category_id, description) VALUES ($1, $2)")
            .bind(category.id)
            .bind(description)
            .execute(&db.pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    for query in [
        "INSERT INTO categories (description) VALUES (NULL)",
        "INSERT INTO tasks (description) VALUES ('Missing category')",
        "INSERT INTO tasks (category_id, description) SELECT id, NULL FROM categories LIMIT 1",
    ] {
        let error = sqlx::query(query).execute(&db.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23502")
        );
    }
    MIGRATOR.run(&db.pool).await.unwrap();
    assert_eq!(
        db.categories().get(category.id).await.unwrap(),
        Some(category)
    );
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(applied, 3);
}

#[tokio::test]
async fn task_urgency_defaults_and_database_constraints() {
    let db = TestDb::new().await;
    let category = db.categories().create("Work").await.unwrap();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tasks (category_id, description) VALUES ($1, 'Existing client') RETURNING id",
    )
    .bind(category.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        db.tasks().get(id).await.unwrap().unwrap().urgency,
        TaskUrgency::Someday
    );
    for (urgency, code) in [
        (Some("invalid"), "23514"),
        (Some(""), "23514"),
        (None, "23502"),
    ] {
        let error = sqlx::query("UPDATE tasks SET urgency = $1 WHERE id = $2")
            .bind(urgency)
            .bind(id)
            .execute(&db.pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some(code)
        );
    }
}

#[tokio::test]
async fn users_have_unique_telegram_ids_and_categories_require_a_valid_owner() {
    use backend::dao::PgUserDao;
    let db = TestDb::new().await;
    let users = PgUserDao::new(db.pool.clone());
    let telegram_id = support::TELEGRAM_ID + 1;
    let (first, concurrent) = tokio::join!(
        users.get_or_create(telegram_id),
        users.get_or_create(telegram_id)
    );
    let first = first.unwrap();
    assert_eq!(first, concurrent.unwrap());
    assert_eq!(first.telegram_id, telegram_id);
    assert_ne!(first.id, telegram_id, "database IDs are surrogate keys");
    assert_ne!(first.id, db.user.id);
    for (query, code) in [
        (
            "INSERT INTO users (telegram_id) SELECT telegram_id FROM users LIMIT 1",
            "23505",
        ),
        ("INSERT INTO users (telegram_id) VALUES (NULL)", "23502"),
        ("INSERT INTO users (telegram_id) VALUES (0)", "23514"),
        (
            "INSERT INTO categories (description) VALUES ('Missing owner')",
            "23502",
        ),
        (
            "INSERT INTO categories (description, user_id) VALUES ('Unknown owner', 999999)",
            "23503",
        ),
    ] {
        let error = sqlx::query(query).execute(&db.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some(code)
        );
    }
    let category = db.categories().create("Owned category").await.unwrap();
    assert_eq!(category.user_id, db.user.id);
    let error = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(db.user.id)
        .execute(&db.pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
}
