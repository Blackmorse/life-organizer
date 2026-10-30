use std::sync::Arc;

use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{
    auth::AuthError,
    dao::{PgCategoryDao, PgDailyDao, PgReminderDao, PgScheduledDao, PgTaskDao, PgUserDao},
    service::{
        CategoryService, DailyService, ReminderService, ScheduledService, ServiceError, TaskService,
    },
};

use super::{AppState, Workspace};

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": self.to_string()})),
        )
            .into_response()
    }
}

pub async fn authenticate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let headers = request.headers().get_all(header::AUTHORIZATION);
    if headers.iter().count() != 1 {
        return AuthError.into_response();
    }
    let telegram_id = headers
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("tma "))
        .ok_or(AuthError)
        .and_then(|data| state.auth.verify(data));
    let telegram_id = match telegram_id {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    let user = match PgUserDao::new(state.pool.clone())
        .get_or_create(telegram_id)
        .await
    {
        Ok(user) => user,
        Err(error) => return ServiceError::from(error).into_response(),
    };
    request.extensions_mut().insert(Workspace {
        reminders: ReminderService::new(Arc::new(PgReminderDao::new(state.pool.clone(), user.id))),
        scheduled: ScheduledService::new(Arc::new(PgScheduledDao::new(
            state.pool.clone(),
            user.id,
        ))),
        daily: DailyService::new(Arc::new(PgDailyDao::new(state.pool.clone(), user.id))),
        categories: CategoryService::new(Arc::new(PgCategoryDao::new(state.pool.clone(), user.id))),
        tasks: TaskService::new(Arc::new(PgTaskDao::new(state.pool.clone(), user.id))),
    });
    request.extensions_mut().insert(user);
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
