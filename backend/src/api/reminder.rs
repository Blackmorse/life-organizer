use super::Workspace;
use crate::{
    domain::{ReminderTask, ReminderTaskInput},
    service::ServiceError,
};
use axum::{Extension, Json, extract::Path, http::StatusCode};
use serde::Deserialize;

pub async fn list(
    Extension(w): Extension<Workspace>,
) -> Result<Json<Vec<ReminderTask>>, ServiceError> {
    Ok(Json(w.reminders.list().await?))
}
pub async fn create(
    Extension(w): Extension<Workspace>,
    Json(input): Json<ReminderTaskInput>,
) -> Result<(StatusCode, Json<ReminderTask>), ServiceError> {
    Ok((
        StatusCode::CREATED,
        Json(w.reminders.save(None, input).await?),
    ))
}
pub async fn update(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<ReminderTaskInput>,
) -> Result<Json<ReminderTask>, ServiceError> {
    Ok(Json(w.reminders.save(Some(id), input).await?))
}
pub async fn delete(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServiceError> {
    w.reminders.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    completed: bool,
}
pub async fn complete(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
    Json(input): Json<Completion>,
) -> Result<Json<ReminderTask>, ServiceError> {
    Ok(Json(w.reminders.complete(id, input.completed).await?))
}
pub async fn dismiss(
    Extension(w): Extension<Workspace>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServiceError> {
    w.reminders.dismiss(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
