use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{domain::Task, service::ServiceError};

use super::AppState;

#[derive(Deserialize)]
pub struct TaskInput {
    category_id: i64,
    description: String,
}

#[derive(Deserialize)]
pub struct TaskFilter {
    category_id: Option<i64>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<TaskInput>,
) -> Result<(StatusCode, Json<Task>), ServiceError> {
    let task = state
        .tasks
        .create(input.category_id, &input.description)
        .await?;
    Ok((StatusCode::CREATED, Json(task)))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Task>, ServiceError> {
    Ok(Json(state.tasks.get(id).await?))
}

pub async fn list(
    State(state): State<AppState>,
    Query(filter): Query<TaskFilter>,
) -> Result<Json<Vec<Task>>, ServiceError> {
    Ok(Json(state.tasks.list(filter.category_id).await?))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<TaskInput>,
) -> Result<Json<Task>, ServiceError> {
    Ok(Json(
        state
            .tasks
            .update(id, input.category_id, &input.description)
            .await?,
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServiceError> {
    state.tasks.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
