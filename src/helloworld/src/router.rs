//! Router for the Notary REST server
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tower_http::trace::TraceLayer;

/// A single notarized receipt.
#[derive(Clone, Debug)]
struct Receipt {
    /// The document hash that was notarized.
    hash: String,
    /// Unix timestamp when the hash was notarized.
    timestamp: u64,
    /// Sequential receipt number.
    receipt_id: u64,
}

/// Shared application state holding all notarized receipts.
#[derive(Debug, Default)]
pub struct AppState {
    /// Map from receipt_id to Receipt.
    receipts: HashMap<u64, Receipt>,
    /// Counter for the next receipt ID.
    next_id: u64,
}

/// Type alias for the shared state.
pub type SharedState = Arc<RwLock<AppState>>;

/// Request body for the notarize endpoint.
#[derive(Deserialize)]
struct NotarizeRequest {
    hash: String,
}

/// Build the application router with all routes.
pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/notarize", post(notarize))
        .route("/verify/{receipt_id}", get(verify))
        .route("/stats", get(stats))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> impl IntoResponse {
    axum::Json(json!({"status": "healthy"}))
}

async fn hello_world() -> impl IntoResponse {
    axum::Json(json!({"message": "hello world"}))
}

async fn time() -> Response {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(now) => (StatusCode::OK, axum::Json(json!({"time": now.as_secs()}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"error": format!("system clock error: {e}")})),
        )
            .into_response(),
    }
}

async fn echo(body: Body) -> Response {
    Response::new(body)
}

async fn notarize(
    State(state): State<SharedState>,
    axum::Json(payload): axum::Json<NotarizeRequest>,
) -> Response {
    if payload.hash.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "hash must not be empty"})),
        )
            .into_response();
    }

    let timestamp = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": format!("system clock error: {e}")})),
            )
                .into_response();
        }
    };

    let mut guard = match state.write() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": format!("lock poisoned: {e}")})),
            )
                .into_response();
        }
    };

    let receipt_id = guard.next_id;
    guard.next_id += 1;

    let receipt = Receipt {
        hash: payload.hash.clone(),
        timestamp,
        receipt_id,
    };
    guard.receipts.insert(receipt_id, receipt);

    (
        StatusCode::OK,
        axum::Json(json!({
            "hash": payload.hash,
            "timestamp": timestamp,
            "receipt_id": receipt_id,
        })),
    )
        .into_response()
}

async fn verify(
    State(state): State<SharedState>,
    Path(receipt_id): Path<u64>,
) -> Response {
    let guard = match state.read() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": format!("lock poisoned: {e}")})),
            )
                .into_response();
        }
    };

    match guard.receipts.get(&receipt_id) {
        Some(receipt) => (
            StatusCode::OK,
            axum::Json(json!({
                "receipt_id": receipt.receipt_id,
                "hash": receipt.hash,
                "timestamp": receipt.timestamp,
            })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "receipt not found"})),
        )
            .into_response(),
    }
}

async fn stats(State(state): State<SharedState>) -> Response {
    let guard = match state.read() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": format!("lock poisoned: {e}")})),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        axum::Json(json!({
            "total_notarized": guard.receipts.len(),
        })),
    )
        .into_response()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_state() -> SharedState {
        Arc::new(RwLock::new(AppState::default()))
    }

    async fn body_string(body: Body) -> String {
        let bytes = body
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
    }

    async fn body_json(body: Body) -> serde_json::Value {
        let s = body_string(body).await;
        serde_json::from_str(&s).expect("response is not valid JSON")
    }

    #[tokio::test]
    async fn test_health() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn test_hello_world() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/hello_world")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["message"], "hello world");
    }

    #[tokio::test]
    async fn test_time() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/time")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert!(json["time"].is_u64(), "time field should be a number");
    }

    #[tokio::test]
    async fn test_echo_text() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/echo")
                    .body(Body::from("hello echo"))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        assert_eq!(body, "hello echo");
    }

    #[tokio::test]
    async fn test_echo_empty() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/echo")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        assert_eq!(body, "");
    }

    #[tokio::test]
    async fn test_echo_json() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/echo")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"foo":"bar"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        assert_eq!(body, r#"{"foo":"bar"}"#);
    }

    #[tokio::test]
    async fn test_notarize_success() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/notarize")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"hash":"abc123"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["hash"], "abc123");
        assert!(json["timestamp"].is_u64());
        assert_eq!(json["receipt_id"], 0);
    }

    #[tokio::test]
    async fn test_notarize_empty_hash_returns_400() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/notarize")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"hash":""}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 400);
        let json = body_json(response.into_body()).await;
        assert!(json["error"].is_string());
    }

    #[tokio::test]
    async fn test_notarize_missing_hash_returns_422() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/notarize")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        // Axum returns 422 for deserialization failures
        assert_eq!(response.status(), 422);
    }

    #[tokio::test]
    async fn test_notarize_sequential_receipt_ids() {
        let state = test_state();

        // First notarize
        let app = router(state.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/notarize")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"hash":"first"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");
        let json = body_json(response.into_body()).await;
        assert_eq!(json["receipt_id"], 0);

        // Second notarize (same state)
        let app = router(state.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/notarize")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"hash":"second"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");
        let json = body_json(response.into_body()).await;
        assert_eq!(json["receipt_id"], 1);
    }

    #[tokio::test]
    async fn test_verify_existing_receipt() {
        let state = test_state();

        // Notarize first
        let app = router(state.clone());
        app.oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/notarize")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"hash":"doc_hash_xyz"}"#))
                .expect("failed to build request"),
        )
        .await
        .expect("failed to execute request");

        // Verify
        let app = router(state.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/verify/0")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["receipt_id"], 0);
        assert_eq!(json["hash"], "doc_hash_xyz");
        assert!(json["timestamp"].is_u64());
    }

    #[tokio::test]
    async fn test_verify_nonexistent_receipt_returns_404() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/verify/999")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 404);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["error"], "receipt not found");
    }

    #[tokio::test]
    async fn test_stats_empty() {
        let app = router(test_state());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/stats")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["total_notarized"], 0);
    }

    #[tokio::test]
    async fn test_stats_after_notarize() {
        let state = test_state();

        // Notarize two documents
        for hash in &["hash1", "hash2"] {
            let app = router(state.clone());
            app.oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/notarize")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"hash":"{hash}"}}"#)))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");
        }

        // Check stats
        let app = router(state.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/stats")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["total_notarized"], 2);
    }
}
