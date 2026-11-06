use super::{AppState, Workspace};
use crate::{
    domain::{DailyTask, User},
    service::ServiceError,
};
use axum::{Extension, Json, extract::State, http::StatusCode};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveInput {
    id: String,
    date: String,
    timezone: Option<String>,
}

pub async fn move_to_date(
    Extension(w): Extension<Workspace>,
    Json(input): Json<MoveInput>,
) -> Result<Json<crate::dao::daily_work::DailyMove>, ServiceError> {
    let item = w
        .daily_work
        .list()
        .await?
        .into_iter()
        .find(|item| item.id == input.id)
        .ok_or(ServiceError::NotFound("current daily item"))?;
    Ok(Json(
        w.daily_work
            .move_to_date(&item, &input.date, input.timezone.as_deref())
            .await?,
    ))
}

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

pub async fn list_tomorrow(
    Extension(w): Extension<Workspace>,
) -> Result<Json<Vec<DailyTask>>, ServiceError> {
    Ok(Json(w.tomorrow.list().await?))
}

pub async fn select_tomorrow(
    Extension(w): Extension<Workspace>,
    Json(input): Json<FutureSelection>,
) -> Result<Json<Vec<DailyTask>>, ServiceError> {
    Ok(Json(
        w.tomorrow
            .select_date(
                input.task_id,
                input.subtask_index,
                &input.timezone,
                input.date.as_deref().unwrap_or("tomorrow"),
            )
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FutureSelection {
    task_id: i64,
    subtask_index: Option<usize>,
    timezone: String,
    date: Option<String>,
}

pub async fn remove_tomorrow(
    Extension(w): Extension<Workspace>,
    Json(input): Json<Removal>,
) -> Result<Json<Vec<DailyTask>>, ServiceError> {
    Ok(Json(
        w.tomorrow.remove(input.task_id, &input.subtask_id).await?,
    ))
}
