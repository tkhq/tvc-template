//! Router for the Hello World REST server
use crate::response::AppError;
use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use qos_core::{EPHEMERAL_KEY_FILE, QUORUM_FILE};
use qos_p256::P256Pair;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use tvc_axum::QosJson;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    ephemeral_key: Arc<P256Pair>,
    quorum_key: Arc<P256Pair>,
}

impl AppState {
    /// Create a new application state value.
    #[must_use]
    pub fn new(ephemeral_key: Arc<P256Pair>, quorum_key: Arc<P256Pair>) -> Self {
        Self {
            ephemeral_key,
            quorum_key,
        }
    }

    /// Create application state by loading keys from hex files once.
    ///
    /// # Errors
    ///
    /// Returns an error if either key file cannot be read or decoded.
    pub fn from_files(
        ephemeral_key_file: impl AsRef<std::path::Path>,
        quorum_key_file: impl AsRef<std::path::Path>,
    ) -> Result<Self, qos_p256::P256Error> {
        Ok(Self::new(
            Arc::new(P256Pair::from_hex_file(ephemeral_key_file)?),
            Arc::new(P256Pair::from_hex_file(quorum_key_file)?),
        ))
    }

    /// Return the loaded ephemeral key for response signing layers.
    #[must_use]
    pub fn ephemeral_key(&self) -> Arc<P256Pair> {
        Arc::clone(&self.ephemeral_key)
    }

    /// Return the loaded quorum key for response signing layers.
    #[must_use]
    pub fn quorum_key(&self) -> Arc<P256Pair> {
        Arc::clone(&self.quorum_key)
    }
}

#[allow(clippy::expect_used)]
impl Default for AppState {
    fn default() -> Self {
        Self::from_files(EPHEMERAL_KEY_FILE, QUORUM_FILE)
            .expect("failed to load default application keys")
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct HelloWorldResponse {
    message: &'static str,
}

#[derive(Serialize)]
struct TimeResponse {
    time: u64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
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
    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/random_app_proof", get(random_app_proof))
        .route("/quorum_key/encrypt", post(quorum_key_encrypt))
        .route("/quorum_key/decrypt", post(quorum_key_decrypt))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    QosJson(HealthResponse { status: "healthy" })
}

async fn hello_world() -> impl IntoResponse {
    QosJson(HelloWorldResponse {
        message: "hello world",
    })
}

async fn time() -> Response {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(now) => (
            StatusCode::OK,
            QosJson(TimeResponse {
                time: now.as_secs(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            QosJson(ErrorResponse {
                error: format!("system clock error: {e}"),
            }),
        )
            .into_response(),
    }
}

async fn echo(body: Body) -> Response {
    Response::new(body)
}

async fn random_app_proof(
    State(state): State<AppState>,
) -> Result<QosJson<RandomAppProofResponse>, AppError> {
    let random_number = rand::random::<u64>();
    let proof_payload = RandomNumberProofPayload { random_number };

    // QOS JSON is a deterministic serialization protocol with stricter rules
    // than normal JSON. It is useful when you need canonical serialization for
    // verifying signatures. We sign these exact bytes and return them in the response
    // to make it easy for clients to verify the signature.
    let payload_bytes = qos_json::to_vec(&proof_payload)
        .map_err(|e| AppError::internal(format!("failed to serialize proof payload: {e}")))?;

    let signature = state
        .ephemeral_key
        .sign(&payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to sign proof payload: {e:?}")))?;
    let payload = String::from_utf8(payload_bytes)
        .map_err(|e| AppError::internal(format!("failed to encode proof payload: {e}")))?;

    let response = RandomAppProofResponse {
        payload: proof_payload,
        proof: AppProof {
            public_key: state.ephemeral_key.public_key().to_bytes(),
            payload,
            signature,
        },
    };

    Ok(QosJson(response))
}

async fn quorum_key_encrypt(
    State(state): State<AppState>,
    Json(request): Json<QuorumKeyEncryptRequest>,
) -> Result<QosJson<QuorumKeyEncryptResponse>, AppError> {
    let ciphertext = state
        .quorum_key
        .public_key()
        .encrypt(request.plaintext.as_bytes())
        .map_err(|e| AppError::internal(format!("failed to encrypt plaintext: {e:?}")))?;

    Ok(QosJson(QuorumKeyEncryptResponse { ciphertext }))
}

async fn quorum_key_decrypt(
    State(state): State<AppState>,
    Json(request): Json<QuorumKeyDecryptRequest>,
) -> Result<QosJson<QuorumKeyDecryptResponse>, AppError> {
    let ciphertext = qos_hex::decode(&request.ciphertext)
        .map_err(|e| AppError::bad_request(format!("invalid ciphertext hex: {e:?}")))?;
    let plaintext = state
        .quorum_key
        .decrypt(&ciphertext)
        .map_err(|e| AppError::bad_request(format!("failed to decrypt ciphertext: {e:?}")))?;
    let plaintext = String::from_utf8(plaintext.to_vec())
        .map_err(|e| AppError::bad_request(format!("decrypted plaintext is not UTF-8: {e}")))?;

    Ok(QosJson(QuorumKeyDecryptResponse { plaintext }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use http_body_util::BodyExt;
    use qos_p256::{P256Pair, P256Public};
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    use tower::ServiceExt;
    use tvc_axum::ResponseSigningLayer;

    const SIGNATURE_COMPONENTS: &str = "(\"@method\" \"@path\" \"@status\" \"content-digest\")";
    const SIGNATURE_ALG: &str = "ecdsa-p256-sha256";

    async fn body_string(body: Body) -> String {
        let bytes = body
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
    }

    fn router_with_temp_keys() -> Router {
        let ephemeral_key =
            Arc::new(P256Pair::generate().expect("failed to generate ephemeral key"));
        let quorum_key = Arc::new(P256Pair::generate().expect("failed to generate quorum key"));
        router_with_state(AppState::new(ephemeral_key, quorum_key))
    }

    fn public_key(key: &P256Pair) -> P256Public {
        P256Public::from_bytes(&key.public_key().to_bytes()).expect("public key should decode")
    }

    fn content_digest(body: &[u8]) -> String {
        format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(body)))
    }

    fn signature_input(label: &str, created: u64) -> String {
        format!(r#"{SIGNATURE_COMPONENTS};created={created};keyid="{label}";alg="{SIGNATURE_ALG}""#)
    }

    fn signature_base(
        method: &str,
        path: &str,
        status: StatusCode,
        digest: &str,
        label: &str,
        created: u64,
    ) -> Vec<u8> {
        format!(
            "\"@method\": {method}\n\"@path\": {path}\n\"@status\": {}\n\"content-digest\": {digest}\n\"@signature-params\": {}",
            status.as_u16(),
            signature_input(label, created)
        )
        .into_bytes()
    }

    fn header_str<'a>(response: &'a Response, name: &str) -> &'a str {
        response
            .headers()
            .get(name)
            .unwrap_or_else(|| panic!("{name} header should exist"))
            .to_str()
            .unwrap_or_else(|_| panic!("{name} header should be ascii"))
    }

    fn label_value<'a>(header: &'a str, label: &str) -> &'a str {
        header
            .split(", ")
            .find_map(|value| value.strip_prefix(&format!("{label}=")))
            .unwrap_or_else(|| panic!("{label} value should exist"))
    }

    fn created_from_signature_input(input: &str, label: &str) -> u64 {
        let value = label_value(input, label);
        let created = value
            .strip_prefix(&format!(r#"{SIGNATURE_COMPONENTS};created="#))
            .and_then(|value| value.split_once(';').map(|(created, _)| created))
            .expect("created parameter should exist");
        created.parse().expect("created should be a unix timestamp")
    }

    fn signature_bytes(signature_header: &str, label: &str) -> Vec<u8> {
        let signature = label_value(signature_header, label)
            .strip_prefix(':')
            .and_then(|value| value.strip_suffix(':'))
            .expect("signature should be an RFC byte sequence");
        STANDARD
            .decode(signature)
            .expect("signature should be base64")
    }

    fn signed_router_with_temp_keys() -> (Router, P256Public, P256Public) {
        let ephemeral_key =
            Arc::new(P256Pair::generate().expect("failed to generate ephemeral key"));
        let quorum_key = Arc::new(P256Pair::generate().expect("failed to generate quorum key"));
        let ephemeral_public_key = public_key(&ephemeral_key);
        let quorum_public_key = public_key(&quorum_key);
        let router = router_with_state(AppState::new(
            Arc::clone(&ephemeral_key),
            Arc::clone(&quorum_key),
        ))
        .layer(
            ResponseSigningLayer::builder()
                .ephemeral_key(ephemeral_key)
                .quorum_key(quorum_key)
                .build(),
        );

        (router, ephemeral_public_key, quorum_public_key)
    }

    async fn signed_body(
        response: Response,
        method: &str,
        path: &str,
        ephemeral_public_key: &P256Public,
        quorum_public_key: &P256Public,
    ) -> Vec<u8> {
        let status = response.status();
        let digest = header_str(&response, "content-digest").to_owned();
        let signature_input_header = header_str(&response, "signature-input").to_owned();
        let signature_header = header_str(&response, "signature").to_owned();
        assert!(!response.headers().contains_key("x-tvc-ephemeral-signature"));
        assert!(!response.headers().contains_key("x-tvc-quorum-signature"));
        assert!(!response.headers().contains_key("x-tvc-signature-timestamp"));
        let created = created_from_signature_input(&signature_input_header, "ephemeral");
        assert_eq!(
            created_from_signature_input(&signature_input_header, "quorum"),
            created
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes()
            .to_vec();

        assert_eq!(digest, content_digest(&body));
        ephemeral_public_key
            .verify(
                &signature_base(method, path, status, &digest, "ephemeral", created),
                &signature_bytes(&signature_header, "ephemeral"),
            )
            .expect("ephemeral response signature should verify");
        quorum_public_key
            .verify(
                &signature_base(method, path, status, &digest, "quorum", created),
                &signature_bytes(&signature_header, "quorum"),
            )
            .expect("quorum response signature should verify");

        body
    }

    #[tokio::test]
    async fn test_health() {
        let app = router_with_temp_keys();
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
        let app = router_with_temp_keys();
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
        let app = router_with_temp_keys();
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
        json["time"]
            .as_str()
            .expect("time field should be a string")
            .parse::<u64>()
            .expect("time field should be a unix timestamp");
    }

    #[tokio::test]
    async fn random_app_proof() {
        let app = router_with_temp_keys();
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
            .as_str()
            .expect("random_number should be a string");
        random_number
            .parse::<u64>()
            .expect("random_number should be a u64");
        let payload = json["proof"]["payload"]
            .as_str()
            .expect("proof payload should be a string");
        let payload_json: serde_json::Value =
            serde_json::from_str(payload).expect("payload is not valid JSON");
        assert_eq!(
            payload_json,
            serde_json::json!({"random_number": random_number})
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
        let app = router_with_temp_keys();
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
        let app = router_with_temp_keys();
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
        let app = router_with_temp_keys();
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
    async fn signs_json_response_body() {
        let (app, ephemeral_public_key, quorum_public_key) = signed_router_with_temp_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = signed_body(
            response,
            "GET",
            "/health",
            &ephemeral_public_key,
            &quorum_public_key,
        )
        .await;
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("response is not valid JSON");
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn signs_echo_response_body() {
        let (app, ephemeral_public_key, quorum_public_key) = signed_router_with_temp_keys();
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

        assert_eq!(response.status(), StatusCode::OK);
        let body = signed_body(
            response,
            "POST",
            "/echo",
            &ephemeral_public_key,
            &quorum_public_key,
        )
        .await;
        assert_eq!(body, b"hello echo");
    }

    #[tokio::test]
    async fn quorum_key_encrypt_and_decrypt_round_trip_utf8_payload() {
        let app = router_with_temp_keys();
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
        let app = router_with_temp_keys();
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
