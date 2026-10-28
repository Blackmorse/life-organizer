mod category;
mod error;
mod task;

use axum::{Router, routing::get};

use crate::service::{CategoryService, TaskService};

#[derive(Clone)]
pub struct AppState {
    pub categories: CategoryService,
    pub tasks: TaskService,
}

pub fn router(state: AppState) -> Router {
    Router::new()
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
        .with_state(state)
}
