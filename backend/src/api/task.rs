use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    domain::{Subtask, Task, TaskUrgency},
    service::ServiceError,
};

use super::Workspace;

#[derive(Deserialize)]
pub struct TaskInput {
    category_id: i64,
    description: String,
    detailed_description: Option<String>,
    #[serde(default)]
    urgency: TaskUrgency,
}

#[derive(Deserialize)]
pub struct TaskFilter {
    category_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct SubtasksInput {
    subtasks: Vec<Subtask>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionInput {
    completed: bool,
}

pub async fn set_completed(
    Extension(state): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<CompletionInput>,
) -> Result<Json<Task>, ServiceError> {
    Ok(Json(state.tasks.set_completed(id, input.completed).await?))
}

pub async fn replace_subtasks(
    Extension(state): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<SubtasksInput>,
) -> Result<Json<Task>, ServiceError> {
    Ok(Json(
        state.tasks.replace_subtasks(id, &input.subtasks).await?,
    ))
}

pub async fn create(
    Extension(state): Extension<Workspace>,
    Json(input): Json<TaskInput>,
) -> Result<(StatusCode, Json<Task>), ServiceError> {
    let task = state
        .tasks
        .create_with_details(
            input.category_id,
            &input.description,
            input.urgency,
            input.detailed_description.as_deref().unwrap_or_default(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(task)))
}

pub async fn get(
    Extension(state): Extension<Workspace>,
    Path(id): Path<i64>,
) -> Result<Json<Task>, ServiceError> {
    Ok(Json(state.tasks.get(id).await?))
}

pub async fn list(
    Extension(state): Extension<Workspace>,
    Query(filter): Query<TaskFilter>,
) -> Result<Json<Vec<Task>>, ServiceError> {
    Ok(Json(state.tasks.list(filter.category_id).await?))
}

pub async fn update(
    Extension(state): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<TaskInput>,
) -> Result<Json<Task>, ServiceError> {
    Ok(Json(
        state
            .tasks
            .update_with_details(
                id,
                input.category_id,
                &input.description,
                input.urgency,
                input.detailed_description.as_deref(),
            )
            .await?,
    ))
}

pub async fn delete(
    Extension(state): Extension<Workspace>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServiceError> {
    state.tasks.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
