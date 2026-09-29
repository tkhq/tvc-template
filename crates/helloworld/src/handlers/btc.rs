use crate::{response::AppError, state::AppState};
use axum::{Json, extract::State};
use serde_json::json;
use std::time::Duration;

const PROVIDER_TIMEOUT: Duration = Duration::from_millis(1200);

struct Provider {
    name: &'static str,
    url: &'static str,
    parse: fn(&serde_json::Value) -> Option<f64>,
}

const PROVIDERS: &[Provider] = &[
    Provider {
        name: "coingecko",
        url: "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd",
        parse: parse_coingecko,
    },
    Provider {
        name: "yahoo-finance",
        url: "https://query1.finance.yahoo.com/v8/finance/chart/BTC-USD?interval=1d&range=1d",
        parse: parse_yahoo,
    },
    Provider {
        name: "coinbase",
        url: "https://api.coinbase.com/v2/prices/BTC-USD/spot",
        parse: parse_coinbase,
    },
];

pub(crate) async fn btc_price(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut reached = false;
    for provider in PROVIDERS {
        match fetch_price(&state, provider, &mut reached).await {
            Ok(price) => {
                return Ok(Json(json!({"bitcoin_usd": price, "source": provider.name})));
            }
            Err(e) => tracing::warn!("{} price request failed: {e}", provider.name),
        }
    }

    if reached {
        return Err(AppError::failed_dependency(
            "no price provider returned a usable price",
        ));
    }
    Err(AppError::internal("could not reach any price provider"))
}

async fn fetch_price(
    state: &AppState,
    provider: &Provider,
    reached: &mut bool,
) -> Result<f64, String> {
    let resp = state
        .http_client
        .get(provider.url)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .inspect(|_| *reached = true)
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?;

    let payload: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    (provider.parse)(&payload).ok_or_else(|| format!("unexpected response: {payload}"))
}

fn parse_coingecko(payload: &serde_json::Value) -> Option<f64> {
    payload
        .get("bitcoin")
        .and_then(|bitcoin| bitcoin.get("usd"))
        .and_then(serde_json::Value::as_f64)
}

fn parse_yahoo(payload: &serde_json::Value) -> Option<f64> {
    payload
        .get("chart")?
        .get("result")?
        .get(0)?
        .get("meta")?
        .get("regularMarketPrice")?
        .as_f64()
}

fn parse_coinbase(payload: &serde_json::Value) -> Option<f64> {
    payload.get("data")?.get("amount")?.as_str()?.parse().ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parses_coingecko_price() {
        let payload = json!({"bitcoin": {"usd": 65000.12}});

        assert_eq!(parse_coingecko(&payload), Some(65000.12));
    }

    #[test]
    fn parses_coingecko_integer_price() {
        let payload = json!({"bitcoin": {"usd": 65000}});

        assert_eq!(parse_coingecko(&payload), Some(65000.0));
    }

    #[test]
    fn parses_yahoo_price() {
        let payload = json!({"chart": {"result": [{"meta": {"regularMarketPrice": 65000.12}}]}});

        assert_eq!(parse_yahoo(&payload), Some(65000.12));
    }

    #[test]
    fn parses_coinbase_price() {
        let payload = json!({"data": {"amount": "65000.12", "base": "BTC", "currency": "USD"}});

        assert_eq!(parse_coinbase(&payload), Some(65000.12));
    }

    #[test]
    fn rejects_missing_or_non_numeric_price() {
        assert_eq!(parse_coingecko(&json!({"ethereum": {"usd": 3200.0}})), None);
        assert_eq!(parse_coingecko(&json!({"bitcoin": {"eur": 60000.0}})), None);
        assert_eq!(parse_coingecko(&json!({"bitcoin": {"usd": "65000"}})), None);
        assert_eq!(parse_yahoo(&json!({"chart": {"result": []}})), None);
        assert_eq!(parse_coinbase(&json!({"data": {"amount": 65000.12}})), None);
        assert_eq!(parse_coinbase(&json!({"data": {"amount": "n/a"}})), None);
    }
}
