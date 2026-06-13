//! Response helpers.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// Application error response.
pub struct AppError {
    status: StatusCode,
    message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

impl AppError {
    /// Create an internal server error.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}
