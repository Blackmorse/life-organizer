mod support;

use backend::domain::TaskUrgency;
use backend::service::ServiceError;
use support::TestDb;

#[tokio::test]
async fn services_validate_input_without_writing_invalid_data() {
    let db = TestDb::new().await;
    let categories = db.category_service();
    let tasks = db.task_service();
    let category = categories.create("Work").await.unwrap();
    let task = tasks
        .create(category.id, "Build backend", TaskUrgency::Soon)
        .await
        .unwrap();
    for description in ["", "  \n\t", "\u{2003}"] {
        assert!(matches!(
            categories.create(description).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            categories.update(category.id, description).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks
                .create(category.id, description, TaskUrgency::Someday)
                .await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks
                .create_with_subtasks(
                    category.id,
                    description,
                    TaskUrgency::Someday,
                    &[backend::domain::Subtask::new("Valid")]
                )
                .await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks
                .create_with_subtasks(
                    category.id,
                    "Valid",
                    TaskUrgency::Someday,
                    &[backend::domain::Subtask::new(description)]
                )
                .await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks.append_subtask(&task, description).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks
                .update(task.id, category.id, description, TaskUrgency::Someday)
                .await,
            Err(ServiceError::InvalidInput(_))
        ));
    }
    for id in [0, -1] {
        let mut invalid_task = task.clone();
        invalid_task.id = id;
        assert!(matches!(
            tasks
                .create_with_subtasks(id, "Valid", TaskUrgency::Someday, &[])
                .await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks.append_subtask(&invalid_task, "Valid").await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            categories.get(id).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            categories.update(id, "Valid").await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            categories.delete(id).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks.get(id).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks.create(id, "Valid", TaskUrgency::Someday).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks
                .update(id, category.id, "Valid", TaskUrgency::Someday)
                .await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks
                .update(task.id, id, "Valid", TaskUrgency::Someday)
                .await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks.delete(id).await,
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            tasks.list(Some(id)).await,
            Err(ServiceError::InvalidInput(_))
        ));
    }
    assert_eq!(categories.list().await.unwrap(), vec![category]);
    assert_eq!(tasks.list(None).await.unwrap(), vec![task]);
}

#[tokio::test]
async fn services_expose_crud_and_translate_missing_rows_and_conflicts() {
    let db = TestDb::new().await;
    let categories = db.category_service();
    let tasks = db.task_service();
    for result in [
        categories.get(999).await,
        categories.update(999, "Missing").await,
    ] {
        assert!(matches!(result, Err(ServiceError::NotFound("category"))));
    }
    assert!(matches!(
        categories.delete(999).await,
        Err(ServiceError::NotFound("category"))
    ));
    assert!(matches!(
        tasks.create(999, "Orphan", TaskUrgency::Someday).await,
        Err(ServiceError::NotFound("category"))
    ));
    let category = categories.create("Work").await.unwrap();
    assert_eq!(categories.get(category.id).await.unwrap(), category);
    let updated = categories.update(category.id, "Home").await.unwrap();
    assert_eq!(updated.description, "Home");
    let task = tasks
        .create(category.id, "Build backend", TaskUrgency::Upcoming)
        .await
        .unwrap();
    assert_eq!(tasks.get(task.id).await.unwrap(), task);
    assert!(matches!(
        categories.delete(category.id).await,
        Err(ServiceError::Conflict)
    ));
    assert!(matches!(
        tasks
            .update(task.id, 999, "Orphan", TaskUrgency::Someday)
            .await,
        Err(ServiceError::NotFound("category"))
    ));
    let updated = tasks
        .update(task.id, category.id, "Write docs", TaskUrgency::Soon)
        .await
        .unwrap();
    assert_eq!(updated.description, "Write docs");
    assert_eq!(updated.urgency, TaskUrgency::Soon);
    assert_eq!(tasks.list(Some(category.id)).await.unwrap(), vec![updated]);
    tasks.delete(task.id).await.unwrap();
    assert!(matches!(
        tasks.get(task.id).await,
        Err(ServiceError::NotFound("task"))
    ));
    assert!(matches!(
        tasks
            .update(task.id, category.id, "Missing", TaskUrgency::Someday)
            .await,
        Err(ServiceError::NotFound("task"))
    ));
    assert!(matches!(
        tasks.delete(task.id).await,
        Err(ServiceError::NotFound("task"))
    ));
    categories.delete(category.id).await.unwrap();
    assert!(categories.list().await.unwrap().is_empty());
}
