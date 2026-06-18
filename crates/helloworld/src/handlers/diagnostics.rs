use crate::state::AppState;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

const RAW_IP_CHECK_URL: &str = "http://1.1.1.1/";
const TLS_IP_CHECK_URL: &str = "https://1.1.1.1/";

pub(crate) async fn raw_ip_check(State(state): State<AppState>) -> Response {
    ip_check("raw_ip_check", &state, RAW_IP_CHECK_URL).await
}

pub(crate) async fn tls_ip_check(State(state): State<AppState>) -> Response {
    ip_check("tls_ip_check", &state, TLS_IP_CHECK_URL).await
}

async fn ip_check(label: &str, state: &AppState, url: &str) -> Response {
    match state.http_client.get(url).send().await {
        Ok(resp) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "requested_url": url,
                "upstream_status": resp.status().as_u16(),
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("{label} request to {url} failed: {e:?}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "ok": false,
                    "requested_url": url,
                    "failure_kind": reqwest_error_kind(&e),
                    "request_error": e.to_string(),
                })),
            )
                .into_response()
        }
    }
}

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
