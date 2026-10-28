use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::service::ServiceError;

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::InvalidInput(_) => StatusCode::BAD_REQUEST,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let error = if let Self::Storage(source) = &self {
            eprintln!("storage error: {source:?}");
            "internal server error".to_owned()
        } else {
            self.to_string()
        };
        (status, Json(ErrorBody { error })).into_response()
    }
}
