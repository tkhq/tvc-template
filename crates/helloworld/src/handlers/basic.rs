use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use tvc_axum::QosJson;

pub(crate) async fn health() -> impl IntoResponse {
    QosJson(json!({"status": "healthy"}))
}

pub(crate) async fn hello_world() -> impl IntoResponse {
    QosJson(json!({"message": "hello world"}))
}

pub(crate) async fn time() -> Response {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(now) => (StatusCode::OK, QosJson(json!({"time": now.as_secs()}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            QosJson(json!({"error": format!("system clock error: {e}")})),
        )
            .into_response(),
    }
}

pub(crate) async fn echo(body: Body) -> Response {
    Response::new(body)
}
