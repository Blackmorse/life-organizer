mod auth;
mod category;
mod error;
mod task;

use axum::{Extension, Json, Router, middleware, routing::get};
use sqlx::PgPool;

use crate::{
    auth::TelegramAuth,
    domain::User,
    service::{CategoryService, TaskService},
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth: TelegramAuth,
}

/// Services are constructed only after verification, with immutable user-scoped DAOs.
#[derive(Clone)]
pub(super) struct Workspace {
    pub categories: CategoryService,
    pub tasks: TaskService,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/me",
            get(|Extension(user): Extension<User>| async { Json(user) }),
        )
        .route("/categories", get(category::list).post(category::create))
        .route(
            "/categories/{id}",
            get(category::get)
                .put(category::update)
                .delete(category::delete),
        )
        .route("/tasks", get(task::list).post(task::create))
        .route(
            "/tasks/{id}",
            get(task::get).put(task::update).delete(task::delete),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::authenticate,
        ))
        .with_state(state)
}
