//! Router for the Hello World REST server
use crate::handlers::{
    btc_price, echo, health, hello_world, quorum_key_decrypt, quorum_key_encrypt, random_app_proof,
    time,
};
use axum::{
    Router,
    routing::{get, post},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub use crate::state::AppState;

/// Build the application router with all routes.
pub fn router() -> Router {
    router_with_state(AppState::default())
}

/// Build the application router with the given state.
pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/btc-price", get(btc_price))
        .route("/random_app_proof", get(random_app_proof))
        .route("/quorum_key/encrypt", post(quorum_key_encrypt))
        .route("/quorum_key/decrypt", post(quorum_key_decrypt))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use qos_core::handles::{EphemeralKeyHandle, QuorumKeyHandle};
    use qos_p256::{P256Pair, P256Public};
    use tower::ServiceExt;

    async fn body_string(body: Body) -> String {
        let bytes = body
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
    }

    fn router_with_temp_keys() -> (Router, tempfile::TempDir) {
        let ephemeral_key = P256Pair::generate().expect("failed to generate ephemeral key");
        let quorum_key = P256Pair::generate().expect("failed to generate quorum key");
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let ephemeral_key_path = temp_dir.path().join("ephemeral.secret");
        let quorum_key_path = temp_dir.path().join("quorum.secret");

        ephemeral_key
            .to_hex_file(&ephemeral_key_path)
            .expect("failed to write ephemeral key");
        quorum_key
            .to_hex_file(&quorum_key_path)
            .expect("failed to write quorum key");

        let app = router_with_state(AppState::new(
            EphemeralKeyHandle::new(
                ephemeral_key_path
                    .to_str()
                    .expect("temp path should be utf8")
                    .to_string(),
            ),
            QuorumKeyHandle::new(
                quorum_key_path
                    .to_str()
                    .expect("temp path should be utf8")
                    .to_string(),
            ),
        ));

        (app, temp_dir)
    }

    #[tokio::test]
    async fn test_health() {
        let app = router();
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
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn test_hello_world() {
        let app = router();
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
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert_eq!(json["message"], "hello world");
    }

    #[tokio::test]
    async fn test_time() {
        let app = router();
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
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert!(json["time"].is_u64(), "time field should be a number");
    }

    #[tokio::test]
    async fn random_app_proof() {
        let (app, _temp_dir) = router_with_temp_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/random_app_proof")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");

        let random_number = json["payload"]["random_number"]
            .as_u64()
            .expect("random_number should be a JSON number");
        let payload = json["proof"]["payload"]
            .as_str()
            .expect("proof payload should be a string");
        let payload_json: serde_json::Value =
            serde_json::from_str(payload).expect("payload is not valid JSON");
        assert_eq!(
            payload_json,
            serde_json::json!({"random_number": random_number.to_string()})
        );

        let public_key = P256Public::from_bytes(
            &qos_hex::decode(
                json["proof"]["public_key"]
                    .as_str()
                    .expect("public key should be a string"),
            )
            .expect("public key should hex decode"),
        )
        .expect("public key should decode");
        let signature = qos_hex::decode(
            json["proof"]["signature"]
                .as_str()
                .expect("signature should be a string"),
        )
        .expect("signature should hex decode");

        public_key
            .verify(payload.as_bytes(), &signature)
            .expect("proof signature should verify");
    }

    #[tokio::test]
    async fn test_echo_text() {
        let app = router();
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
        let app = router();
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
        let app = router();
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
    async fn btc_price_route_rejects_unsupported_methods() {
        let app = router();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/btc-price")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn quorum_key_encrypt_and_decrypt_round_trip_utf8_payload() {
        let (app, _temp_dir) = router_with_temp_keys();
        let plaintext = "hello TVC world";
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/quorum_key/encrypt")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"plaintext":"{plaintext}"}}"#)))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        let ciphertext = json["ciphertext"]
            .as_str()
            .expect("ciphertext should be a string");
        qos_hex::decode(ciphertext).expect("ciphertext should be hex");

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/quorum_key/decrypt")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"ciphertext":"{ciphertext}"}}"#)))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert_eq!(json["plaintext"], plaintext);
    }

    #[tokio::test]
    async fn quorum_key_decrypt_rejects_malformed_ciphertext_hex() {
        let (app, _temp_dir) = router_with_temp_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/quorum_key/decrypt")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"ciphertext":"not-hex"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
