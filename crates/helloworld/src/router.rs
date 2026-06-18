//! Router for the Hello World REST server
use crate::response::AppError;
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
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

/// CoinGecko simple-price endpoint for the current Bitcoin price in USD.
const COINGECKO_BTC_PRICE_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd";

/// Upper bound on a single outbound egress request. Without this the request can
/// hang inside the enclave's verified egress, in which case the qos client in
/// front of the app times out first and returns a bare `error code: 502` —
/// hiding the JSON error below. This MUST be shorter than the qos manifest's
/// `clientTimeoutMs` so the app fails first and can surface a descriptive error.
const EGRESS_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    ephemeral_key_handle: EphemeralKeyHandle<String>,
    quorum_key_handle: QuorumKeyHandle,
    http_client: reqwest::Client,
}

impl AppState {
    /// Create a new application state value.
    #[must_use]
    pub fn new(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
    ) -> Self {
        Self::new_with_http_client(
            ephemeral_key_handle,
            quorum_key_handle,
            http_client(),
        )
    }

    fn new_with_http_client(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
        http_client: reqwest::Client,
    ) -> Self {
        Self {
            ephemeral_key_handle,
            quorum_key_handle,
            http_client,
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

fn http_client() -> reqwest::Client {
    match reqwest::Client::builder()
        .use_rustls_tls()
        .user_agent(concat!("tvc-helloworld/", env!("CARGO_PKG_VERSION")))
        .timeout(EGRESS_REQUEST_TIMEOUT)
        .connect_timeout(EGRESS_REQUEST_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("failed to build CoinGecko HTTP client: {e}");
            reqwest::Client::new()
        }
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
    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/btc_price", get(btc_price))
        .route("/raw_ip_check", get(raw_ip_check))
        .route("/random_app_proof", get(random_app_proof))
        .route("/quorum_key/encrypt", post(quorum_key_encrypt))
        .route("/quorum_key/decrypt", post(quorum_key_decrypt))
        .layer(
            // Log every request and its response at INFO. The defaults emit at
            // DEBUG, which is invisible under the default `info` env filter.
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
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

async fn btc_price(State(state): State<AppState>) -> Response {
    let resp = match state.http_client.get(COINGECKO_BTC_PRICE_URL).send().await {
        Ok(resp) => resp,
        Err(e) => {
            let kind = reqwest_error_kind(&e);
            // `{e:?}` includes the underlying source chain (DNS/TLS/IO), which is
            // far more useful than the `Display` form when diagnosing egress.
            tracing::error!("coingecko request failed ({kind}): {e:?}");
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({
                    "error": "failed to reach price provider",
                    "failure_kind": kind,
                    "coingecko_error": e.to_string(),
                    "coingecko_url": COINGECKO_BTC_PRICE_URL,
                })),
            )
                .into_response();
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let upstream_error = match resp.text().await {
            Ok(body) => body,
            Err(e) => format!("failed to read coingecko error response: {e}"),
        };
        tracing::error!("coingecko returned non-success status {status}: {upstream_error}");
        return (
            StatusCode::BAD_GATEWAY,
            axum::Json(coingecko_error_json(status, &upstream_error)),
        )
            .into_response();
    }

    let payload: serde_json::Value = match resp.json().await {
        Ok(payload) => payload,
        Err(e) => {
            tracing::error!("failed to parse coingecko response: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({
                    "error": "failed to parse price provider response",
                    "coingecko_error": e.to_string(),
                })),
            )
                .into_response();
        }
    };

    match parse_btc_usd(&payload) {
        Some(price) => (StatusCode::OK, axum::Json(json!({"bitcoin_usd": price}))).into_response(),
        None => {
            tracing::error!("coingecko response missing bitcoin.usd field: {payload}");
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({"error": "unexpected price provider response"})),
            )
                .into_response()
        }
    }
}

/// Classify a `reqwest` failure into a short, stable label so clients and logs
/// can tell a timeout apart from a connection or TLS failure without parsing the
/// free-form error string.
fn reqwest_error_kind(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else if e.is_redirect() {
        "redirect"
    } else if e.is_decode() {
        "decode"
    } else if e.is_body() {
        "body"
    } else {
        "other"
    }
}

/// A well-known, stable Cloudflare anycast IP that answers plain HTTP on port
/// 80. Using a literal IP (not a hostname) over `http://` (not `https://`)
/// isolates the enclave's raw egress data path from both DNS resolution and TLS:
/// if `/raw_ip_check` succeeds but `/btc_price` fails, the problem is DNS or TLS,
/// not raw TCP egress.
const RAW_IP_CHECK_URL: &str = "http://1.1.1.1/";

/// Probe raw TCP egress by issuing a plain-HTTP request to a hardcoded IP
/// (`RAW_IP_CHECK_URL`). Query params are not used because they don't survive
/// the TVC ingress path. Redirects are disabled so a `3xx` from the raw IP does
/// not trigger a follow-up request to a hostname (which would reintroduce DNS).
///
/// Any HTTP status returned (including a redirect) proves raw egress works and
/// yields `200 OK` with the upstream status in `status`. A transport-level
/// failure (connect/timeout) returns `502` with a `failure_kind` label.
async fn raw_ip_check() -> Response {
    // A dedicated client: redirects disabled so we never resolve a hostname, and
    // the shared egress timeout so this fails fast like the other egress routes.
    let client = match reqwest::Client::builder()
        .use_rustls_tls()
        .user_agent(concat!("tvc-helloworld/", env!("CARGO_PKG_VERSION")))
        .timeout(EGRESS_REQUEST_TIMEOUT)
        .connect_timeout(EGRESS_REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("raw_ip_check failed to build http client: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({
                    "error": "failed to build http client",
                    "request_error": e.to_string(),
                })),
            )
                .into_response();
        }
    };

    match client.get(RAW_IP_CHECK_URL).send().await {
        Ok(resp) => (
            StatusCode::OK,
            axum::Json(json!({
                "ok": true,
                "requested_url": RAW_IP_CHECK_URL,
                "status": resp.status().as_u16(),
                "note": "any status here means raw-TCP egress works (DNS and TLS were bypassed)",
            })),
        )
            .into_response(),
        Err(e) => {
            let kind = reqwest_error_kind(&e);
            tracing::error!("raw_ip_check request to {RAW_IP_CHECK_URL} failed ({kind}): {e:?}");
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({
                    "ok": false,
                    "requested_url": RAW_IP_CHECK_URL,
                    "failure_kind": kind,
                    "request_error": e.to_string(),
                })),
            )
                .into_response()
        }
    }
}

fn parse_btc_usd(payload: &serde_json::Value) -> Option<f64> {
    payload
        .get("bitcoin")
        .and_then(|b| b.get("usd"))
        .and_then(serde_json::Value::as_f64)
}

fn coingecko_error_json(status: StatusCode, upstream_error: &str) -> serde_json::Value {
    json!({
        "error": "price provider returned an error",
        "upstream_status": status.as_u16(),
        "upstream_error": upstream_error,
    })
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

        let app = router_with_state(AppState::new_with_http_client(
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
            http_client(),
        ));

        (app, temp_dir)
    }

    #[test]
    fn app_state_uses_supplied_http_client() {
        let client = reqwest::Client::new();
        let state = AppState::new_with_http_client(
            EphemeralKeyHandle::new("ephemeral.secret".to_string()),
            QuorumKeyHandle::new("quorum.secret".to_string()),
            client,
        );

        let request = state
            .http_client
            .get(COINGECKO_BTC_PRICE_URL)
            .build()
            .expect("request should build");
        assert_eq!(request.url().as_str(), COINGECKO_BTC_PRICE_URL);
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

    #[test]
    fn test_parse_btc_usd_valid() {
        let payload = serde_json::json!({"bitcoin": {"usd": 65000.12}});
        assert_eq!(parse_btc_usd(&payload), Some(65000.12));
    }

    #[test]
    fn test_parse_btc_usd_integer_price() {
        let payload = serde_json::json!({"bitcoin": {"usd": 65000}});
        assert_eq!(parse_btc_usd(&payload), Some(65000.0));
    }

    #[test]
    fn test_parse_btc_usd_missing_asset() {
        let payload = serde_json::json!({"ethereum": {"usd": 3200.0}});
        assert_eq!(parse_btc_usd(&payload), None);
    }

    #[test]
    fn test_parse_btc_usd_missing_currency() {
        let payload = serde_json::json!({"bitcoin": {"eur": 60000.0}});
        assert_eq!(parse_btc_usd(&payload), None);
    }

    #[test]
    fn test_parse_btc_usd_wrong_type() {
        let payload = serde_json::json!({"bitcoin": {"usd": "not-a-number"}});
        assert_eq!(parse_btc_usd(&payload), None);
    }

    #[test]
    fn test_parse_btc_usd_empty() {
        let payload = serde_json::json!({});
        assert_eq!(parse_btc_usd(&payload), None);
    }

    #[test]
    fn coingecko_error_json_includes_upstream_status_and_error() {
        let json = coingecko_error_json(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");

        assert_eq!(json["error"], "price provider returned an error");
        assert_eq!(json["upstream_status"], 429);
        assert_eq!(json["upstream_error"], "rate limit exceeded");
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
