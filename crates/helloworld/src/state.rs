//! Shared server state.

use crate::client::HttpClient;
use qos_p256::P256Pair;
use std::sync::Arc;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) ephemeral_key: Arc<P256Pair>,
    pub(crate) quorum_key: Arc<P256Pair>,
    pub(crate) http_client: HttpClient,
}

impl AppState {
    /// Create a new application state value.
    pub fn new(ephemeral_key: P256Pair, quorum_key: P256Pair) -> Result<Self, reqwest::Error> {
        Ok(Self::new_with_http_client(
            ephemeral_key,
            quorum_key,
            HttpClient::new()?,
        ))
    }

    pub(crate) fn new_with_http_client(
        ephemeral_key: P256Pair,
        quorum_key: P256Pair,
        http_client: HttpClient,
    ) -> Self {
        Self {
            ephemeral_key: Arc::new(ephemeral_key),
            quorum_key: Arc::new(quorum_key),
            http_client,
        }
    }
}
