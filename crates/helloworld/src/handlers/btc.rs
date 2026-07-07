use crate::state::AppState;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

const COINGECKO_BTC_PRICE_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd";

pub(crate) async fn btc_price(State(state): State<AppState>) -> Response {
    let resp = match state.http_client.get(COINGECKO_BTC_PRICE_URL).send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("coingecko request failed: {e:?}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "failed to reach price provider",
                    "coingecko_error": e.to_string(),
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
            Json(coingecko_error_json(status, &upstream_error)),
        )
            .into_response();
    }

    let payload: serde_json::Value = match resp.json().await {
        Ok(payload) => payload,
        Err(e) => {
            tracing::error!("failed to parse coingecko response: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "failed to parse price provider response",
                    "coingecko_error": e.to_string(),
                })),
            )
                .into_response();
        }
    };

    match parse_btc_usd(&payload) {
        Some(price) => (StatusCode::OK, Json(json!({"bitcoin_usd": price}))).into_response(),
        None => {
            tracing::error!("coingecko response missing bitcoin.usd field: {payload}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "unexpected price provider response"})),
            )
                .into_response()
        }
    }
}

fn parse_btc_usd(payload: &serde_json::Value) -> Option<f64> {
    payload
        .get("bitcoin")
        .and_then(|bitcoin| bitcoin.get("usd"))
        .and_then(serde_json::Value::as_f64)
}

fn coingecko_error_json(status: StatusCode, upstream_error: &str) -> serde_json::Value {
    json!({
        "error": "price provider returned an error",
        "upstream_status": status.as_u16(),
        "upstream_error": upstream_error,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parses_float_price() {
        let payload = json!({"bitcoin": {"usd": 65000.12}});

        assert_eq!(parse_btc_usd(&payload), Some(65000.12));
    }

    #[test]
    fn parses_integer_price() {
        let payload = json!({"bitcoin": {"usd": 65000}});

        assert_eq!(parse_btc_usd(&payload), Some(65000.0));
    }

    #[test]
    fn rejects_missing_or_non_numeric_price() {
        assert_eq!(parse_btc_usd(&json!({"ethereum": {"usd": 3200.0}})), None);
        assert_eq!(parse_btc_usd(&json!({"bitcoin": {"eur": 60000.0}})), None);
        assert_eq!(parse_btc_usd(&json!({"bitcoin": {"usd": "65000"}})), None);
    }

    #[test]
    fn upstream_error_json_includes_status_and_body() {
        let body = coingecko_error_json(StatusCode::TOO_MANY_REQUESTS, "rate limit");

        assert_eq!(body["error"], "price provider returned an error");
        assert_eq!(body["upstream_status"], 429);
        assert_eq!(body["upstream_error"], "rate limit");
    }
}
