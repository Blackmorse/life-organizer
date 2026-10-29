use super::Workspace;
use crate::{
    domain::{ScheduledOccurrence, ScheduledTask, ScheduledTaskInput, Subtask},
    service::ServiceError,
};
use axum::{Extension, Json, extract::Path, http::StatusCode};
use serde::Deserialize;

pub async fn list(
    Extension(w): Extension<Workspace>,
) -> Result<Json<Vec<ScheduledTask>>, ServiceError> {
    Ok(Json(w.scheduled.list().await?))
}
pub async fn create(
    Extension(w): Extension<Workspace>,
    Json(input): Json<ScheduledTaskInput>,
) -> Result<(StatusCode, Json<ScheduledTask>), ServiceError> {
    Ok((
        StatusCode::CREATED,
        Json(w.scheduled.save(None, input).await?),
    ))
}
pub async fn update(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<ScheduledTaskInput>,
) -> Result<Json<ScheduledTask>, ServiceError> {
    Ok(Json(w.scheduled.save(Some(id), input).await?))
}
pub async fn delete(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServiceError> {
    w.scheduled.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn daily(
    Extension(w): Extension<Workspace>,
) -> Result<Json<Vec<ScheduledOccurrence>>, ServiceError> {
    Ok(Json(w.scheduled.daily().await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    completed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steps {
    subtasks: Vec<Subtask>,
}
pub async fn complete(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<Completion>,
) -> Result<Json<Vec<ScheduledOccurrence>>, ServiceError> {
    Ok(Json(
        w.scheduled
            .progress(id, Some(input.completed), None)
            .await?,
    ))
}
pub async fn subtasks(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<Steps>,
) -> Result<Json<Vec<ScheduledOccurrence>>, ServiceError> {
    Ok(Json(
        w.scheduled.progress(id, None, Some(input.subtasks)).await?,
    ))
}
pub async fn dismiss(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<ScheduledOccurrence>>, ServiceError> {
    Ok(Json(w.scheduled.dismiss(id).await?))
}
