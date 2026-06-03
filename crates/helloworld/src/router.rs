//! Router for the Hello World REST server
use axum::{
    Router,
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;
use tower_http::trace::TraceLayer;

/// CoinGecko simple-price endpoint for the current Bitcoin price in USD.
const COINGECKO_BTC_PRICE_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd";

/// Build the application router with all routes.
pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/btc-price", get(btc_price))
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

/// Fetch the current Bitcoin price in USD from CoinGecko and return it as JSON.
///
/// The outbound request is made with a `reqwest` client backed by `rustls-tls`,
/// so the TLS handshake and server-certificate verification happen in-process
/// inside the enclave (QuorumOS verified egress). The enclave ships without
/// system SSL libraries, so the OpenSSL/native-tls backend is intentionally not
/// linked.
///
/// On success the response is `{"bitcoin_usd": <price>}`. Upstream/transport
/// failures map to `502 Bad Gateway`; a malformed upstream payload maps to
/// `502 Bad Gateway` as well, since the fault is with the external API.
async fn btc_price() -> Response {
    // Build a fresh rustls-backed client per request. This keeps the handler
    // self-contained and avoids shared mutable state; for higher throughput a
    // shared client could be stored in router state instead.
    let client = match reqwest::Client::builder()
        .use_rustls_tls()
        // Some upstreams (CoinGecko included) reject requests without an
        // explicit User-Agent with HTTP 403, so set one here.
        .user_agent(concat!("tvc-helloworld/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("failed to build HTTP client: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"error": "failed to build HTTP client"})),
            )
                .into_response();
        }
    };

    let resp = match client.get(COINGECKO_BTC_PRICE_URL).send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("coingecko request failed: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({"error": "failed to reach price provider"})),
            )
                .into_response();
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        tracing::error!("coingecko returned non-success status: {status}");
        return (
            StatusCode::BAD_GATEWAY,
            axum::Json(json!({
                "error": "price provider returned an error",
                "upstream_status": status.as_u16(),
            })),
        )
            .into_response();
    }

    let payload: serde_json::Value = match resp.json().await {
        Ok(payload) => payload,
        Err(e) => {
            tracing::error!("failed to parse coingecko response: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({"error": "failed to parse price provider response"})),
            )
                .into_response();
        }
    };

    // Expected shape: {"bitcoin": {"usd": <number>}}
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

/// Extract the Bitcoin/USD price from a CoinGecko `simple/price` response body.
///
/// Returns `None` if the expected `bitcoin.usd` numeric field is absent.
fn parse_btc_usd(payload: &serde_json::Value) -> Option<f64> {
    payload
        .get("bitcoin")
        .and_then(|b| b.get("usd"))
        .and_then(serde_json::Value::as_f64)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn body_string(body: Body) -> String {
        let bytes = body
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
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
}
