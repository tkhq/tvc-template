use crate::{response::AppError, state::AppState};
use axum::{Json, extract::State};
use serde_json::json;
use std::{error::Error, net::IpAddr, time::Duration};

const PROVIDER_TIMEOUT: Duration = Duration::from_secs(2);

const PROVIDERS: &[&str] = &[
    "https://checkip.amazonaws.com",
    "https://api.ipify.org",
    "https://icanhazip.com",
];

pub(crate) async fn check_ip(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut errors = Vec::new();
    for url in PROVIDERS {
        match fetch_ip(&state, url).await {
            Ok(ip) => return Ok(Json(json!({"ip": ip, "source": url}))),
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }
    Err(AppError::bad_gateway(errors.join("; ")))
}

async fn fetch_ip(state: &AppState, url: &str) -> Result<IpAddr, String> {
    let body = state
        .http_client
        .get(url)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(error_chain)?
        .text()
        .await
        .map_err(error_chain)?;

    body.trim()
        .parse()
        .map_err(|_| format!("unexpected response: {body}"))
}

/// DNS, TLS, and timeout failures differ only in the source chain.
fn error_chain(e: reqwest::Error) -> String {
    let mut message = e.to_string();
    for cause in std::iter::successors(e.source(), |&cause| cause.source()) {
        message.push_str(&format!(": {cause}"));
    }
    message
}
