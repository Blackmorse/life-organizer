mod auth;
mod category;
mod daily;
mod error;
mod scheduled;
mod task;

use axum::{Extension, Json, Router, middleware, routing::get};
use sqlx::PgPool;
use std::sync::Arc;

use crate::{
    auth::TelegramAuth,
    domain::User,
    service::{CategoryService, DailyService, MessageSender, ScheduledService, TaskService},
};

#[derive(Clone)]
pub struct AppState {
    pub notifications: Arc<dyn MessageSender>,
    pub pool: PgPool,
    pub auth: TelegramAuth,
}

/// Services are constructed only after verification, with immutable user-scoped DAOs.
#[derive(Clone)]
pub(super) struct Workspace {
    pub scheduled: ScheduledService,
    pub daily: DailyService,
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
            "/scheduled-tasks",
            get(scheduled::list).post(scheduled::create),
        )
        .route(
            "/scheduled-tasks/{id}",
            axum::routing::put(scheduled::update).delete(scheduled::delete),
        )
        .route("/scheduled-tasks/daily", get(scheduled::daily))
        .route(
            "/scheduled-tasks/daily/{id}",
            axum::routing::delete(scheduled::dismiss),
        )
        .route(
            "/scheduled-tasks/daily/{id}/completion",
            axum::routing::put(scheduled::complete),
        )
        .route(
            "/scheduled-tasks/daily/{id}/subtasks",
            axum::routing::put(scheduled::subtasks),
        )
        .route(
            "/daily-tasks",
            get(daily::list).post(daily::select).delete(daily::remove),
        )
        .route(
            "/daily-tasks/finalize",
            axum::routing::post(daily::finalize),
        )
        .route(
            "/tasks/{id}/completion",
            axum::routing::put(task::set_completed),
        )
        .route(
            "/tasks/{id}/subtasks",
            axum::routing::put(task::replace_subtasks),
        )
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
