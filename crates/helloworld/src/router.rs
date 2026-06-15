//! Router for the Hello World REST server
use crate::response::AppError;
use crate::signing::ResponseSigningLayer;
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use qos_core::{
    EPHEMERAL_KEY_FILE, QUORUM_FILE,
    handles::{EphemeralKeyHandle, QuorumKeyHandle},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::trace::TraceLayer;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    ephemeral_key_handle: EphemeralKeyHandle<String>,
    quorum_key_handle: QuorumKeyHandle,
}

impl AppState {
    /// Create a new application state value.
    #[must_use]
    pub fn new(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
    ) -> Self {
        Self {
            ephemeral_key_handle,
            quorum_key_handle,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new(
            EphemeralKeyHandle::new(EPHEMERAL_KEY_FILE.to_string()),
            QuorumKeyHandle::new(QUORUM_FILE.to_string()),
        )
    }
}

#[derive(Serialize)]
struct RandomNumberProofPayload {
    // Additional metadata can be added here later if the proof needs stronger
    // domain separation or audit context.
    #[serde(with = "qos_json::string_or_numeric")]
    random_number: u64,
}

#[derive(Serialize)]
struct AppProof {
    // The enclave's ephemeral public key used to generate the signature.
    #[serde(with = "qos_hex::serde")]
    public_key: Vec<u8>,
    // The exact serialized payload is included so clients can verify the
    // signature without extra deterministic serialization logic.
    payload: String,
    // The ephemeral key's signature over the payload.
    #[serde(with = "qos_hex::serde")]
    signature: Vec<u8>,
}

#[derive(Serialize)]
struct RandomAppProofResponse {
    payload: RandomNumberProofPayload,
    proof: AppProof,
}

#[derive(Deserialize)]
struct QuorumKeyEncryptRequest {
    plaintext: String,
}

#[derive(Serialize)]
struct QuorumKeyEncryptResponse {
    #[serde(with = "qos_hex::serde")]
    ciphertext: Vec<u8>,
}

#[derive(Deserialize)]
struct QuorumKeyDecryptRequest {
    ciphertext: String,
}

#[derive(Serialize)]
struct QuorumKeyDecryptResponse {
    plaintext: String,
}

/// Build the application router with all routes.
pub fn router() -> Router {
    router_with_state(AppState::default())
}

/// Build the application router with the given state.
pub fn router_with_state(state: AppState) -> Router {
    // Sign every response body with the same ephemeral qos_p256 key that the
    // application state exposes (the key used by `random_app_proof`).
    let signing_layer = ResponseSigningLayer::new(state.ephemeral_key_handle.clone());

    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/random_app_proof", get(random_app_proof))
        .route("/quorum_key/encrypt", post(quorum_key_encrypt))
        .route("/quorum_key/decrypt", post(quorum_key_decrypt))
        .layer(signing_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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

async fn random_app_proof(
    State(state): State<AppState>,
) -> Result<Json<RandomAppProofResponse>, AppError> {
    let random_number = rand::random::<u64>();
    let proof_payload = RandomNumberProofPayload { random_number };

    // QOS JSON is a deterministic serialization protocol with stricter rules
    // than normal JSON. It is useful when you need canonical serialization for
    // verifying signatures. We sign these exact bytes and return them in the response
    // to make it easy for clients to verify the signature.
    let payload_bytes = qos_json::to_vec(&proof_payload)
        .map_err(|e| AppError::internal(format!("failed to serialize proof payload: {e}")))?;

    let ephemeral_key = state
        .ephemeral_key_handle
        .get_ephemeral_key()
        .map_err(|e| AppError::internal(format!("failed to load ephemeral key: {e}")))?;
    let signature = ephemeral_key
        .sign(&payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to sign proof payload: {e:?}")))?;
    let payload = String::from_utf8(payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to encode proof payload: {e}")))?;

    let response = RandomAppProofResponse {
        payload: proof_payload,
        proof: AppProof {
            public_key: ephemeral_key.public_key().to_bytes(),
            payload,
            signature,
        },
    };

    Ok(Json(response))
}

async fn quorum_key_encrypt(
    State(state): State<AppState>,
    Json(request): Json<QuorumKeyEncryptRequest>,
) -> Result<Json<QuorumKeyEncryptResponse>, AppError> {
    let quorum_key = state
        .quorum_key_handle
        .get_quorum_key()
        .map_err(|e| AppError::internal(format!("failed to load quorum key: {e}")))?;
    let ciphertext = quorum_key
        .public_key()
        .encrypt(request.plaintext.as_bytes())
        .map_err(|e| AppError::internal(format!("failed to encrypt plaintext: {e:?}")))?;

    Ok(Json(QuorumKeyEncryptResponse { ciphertext }))
}

async fn quorum_key_decrypt(
    State(state): State<AppState>,
    Json(request): Json<QuorumKeyDecryptRequest>,
) -> Result<Json<QuorumKeyDecryptResponse>, AppError> {
    let ciphertext = qos_hex::decode(&request.ciphertext)
        .map_err(|e| AppError::bad_request(format!("invalid ciphertext hex: {e:?}")))?;
    let quorum_key = state
        .quorum_key_handle
        .get_quorum_key()
        .map_err(|e| AppError::internal(format!("failed to load quorum key: {e}")))?;
    let plaintext = quorum_key
        .decrypt(&ciphertext)
        .map_err(|e| AppError::bad_request(format!("failed to decrypt ciphertext: {e:?}")))?;
    let plaintext = String::from_utf8(plaintext.to_vec())
        .map_err(|e| AppError::bad_request(format!("decrypted plaintext is not UTF-8: {e}")))?;

    Ok(Json(QuorumKeyDecryptResponse { plaintext }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
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
