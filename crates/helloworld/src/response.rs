//! Response helpers for QOS canonical JSON.

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// JSON response serialized with QOS canonical JSON.
pub struct QosJson<T>(pub T);

impl<T> IntoResponse for QosJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        match qos_json::to_vec(&self.0) {
            Ok(bytes) => {
                let mut response = Response::new(Body::from(bytes));
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/json"),
                );
                response
            }
            Err(error) => {
                AppError::internal(format!("failed to serialize response: {error}")).into_response()
            }
        }
    }
}

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
        let mut response = QosJson(ErrorResponse {
            error: self.message,
        })
        .into_response();
        *response.status_mut() = self.status;
        response
    }
}
