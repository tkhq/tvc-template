use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub(crate) async fn health() -> impl IntoResponse {
    axum::Json(json!({"status": "healthy"}))
}

pub(crate) async fn hello_world() -> impl IntoResponse {
    axum::Json(json!({"message": "hello world"}))
}

pub(crate) async fn time() -> Response {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(now) => (StatusCode::OK, axum::Json(json!({"time": now.as_secs()}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"error": format!("system clock error: {e}")})),
        )
            .into_response(),
    }
}

pub(crate) async fn echo(body: Body) -> Response {
    Response::new(body)
}
