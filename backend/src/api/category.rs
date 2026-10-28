use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{domain::Category, service::ServiceError};

use super::AppState;

#[derive(Deserialize)]
pub struct CategoryInput {
    description: String,
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<CategoryInput>,
) -> Result<(StatusCode, Json<Category>), ServiceError> {
    let category = state.categories.create(&input.description).await?;
    Ok((StatusCode::CREATED, Json(category)))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Category>, ServiceError> {
    Ok(Json(state.categories.get(id).await?))
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Category>>, ServiceError> {
    Ok(Json(state.categories.list().await?))
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<CategoryInput>,
) -> Result<Json<Category>, ServiceError> {
    Ok(Json(state.categories.update(id, &input.description).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServiceError> {
    state.categories.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
