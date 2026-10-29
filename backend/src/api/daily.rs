use super::{AppState, Workspace};
use crate::{
    domain::{DailyTask, User},
    service::ServiceError,
};
use axum::{Extension, Json, extract::State, http::StatusCode};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    task_id: i64,
    subtask_index: Option<usize>,
    timezone: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removal {
    task_id: i64,
    subtask_id: String,
}
pub async fn finalize(
    State(state): State<AppState>,
    Extension(w): Extension<Workspace>,
    Extension(user): Extension<User>,
) -> Result<StatusCode, ServiceError> {
    w.daily
        .finalize(user.telegram_id, state.notifications.as_ref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn list(
    Extension(w): Extension<Workspace>,
) -> Result<Json<Vec<DailyTask>>, ServiceError> {
    Ok(Json(w.daily.list().await?))
}
pub async fn select(
    Extension(w): Extension<Workspace>,
    Json(input): Json<Selection>,
) -> Result<Json<Vec<DailyTask>>, ServiceError> {
    Ok(Json(
        w.daily
            .select(input.task_id, input.subtask_index, &input.timezone)
            .await?,
    ))
}
pub async fn remove(
    Extension(w): Extension<Workspace>,
    Json(input): Json<Removal>,
) -> Result<Json<Vec<DailyTask>>, ServiceError> {
    Ok(Json(
        w.daily.remove(input.task_id, &input.subtask_id).await?,
    ))
}
