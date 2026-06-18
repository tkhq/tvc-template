//! Reusable outbound HTTP client.

use std::time::Duration;

const EGRESS_REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone)]
pub(crate) struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub(crate) fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .user_agent(concat!("tvc-helloworld/", env!("CARGO_PKG_VERSION")))
            .timeout(EGRESS_REQUEST_TIMEOUT)
            .connect_timeout(EGRESS_REQUEST_TIMEOUT)
            .build()?;

        Ok(Self { client })
    }

    pub(crate) fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.get(url)
    }
}
