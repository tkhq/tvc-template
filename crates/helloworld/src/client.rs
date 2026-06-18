//! Reusable outbound HTTP client.

use std::time::Duration;

const EGRESS_REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone)]
pub(crate) struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub(crate) fn new() -> Self {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .user_agent(concat!("tvc-helloworld/", env!("CARGO_PKG_VERSION")))
            .timeout(EGRESS_REQUEST_TIMEOUT)
            .connect_timeout(EGRESS_REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("failed to build HTTP client: {e}");
                reqwest::Client::new()
            });

        Self { client }
    }

    pub(crate) fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.get(url)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn builds_requests_for_different_urls() {
        let client = HttpClient::new();

        let coingecko = client
            .get("https://api.coingecko.com/api/v3/ping")
            .build()
            .expect("request should build");
        let cloudflare = client
            .get("https://1.1.1.1/")
            .build()
            .expect("request should build");

        assert_eq!(
            coingecko.url().as_str(),
            "https://api.coingecko.com/api/v3/ping"
        );
        assert_eq!(cloudflare.url().as_str(), "https://1.1.1.1/");
    }
}
