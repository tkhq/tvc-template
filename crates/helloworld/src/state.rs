//! Shared server state.

use crate::client::HttpClient;
use qos_p256::P256Pair;
use std::sync::Arc;
use turnkey_api_key_stamper::TurnkeyP256ApiKey;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) ephemeral_key: Arc<P256Pair>,
    pub(crate) quorum_key: Arc<P256Pair>,
    pub(crate) turnkey_api_key: Arc<TurnkeyP256ApiKey>,
    pub(crate) http_client: HttpClient,
}

impl AppState {
    /// Create a new application state value.
    pub fn new(
        ephemeral_key: P256Pair,
        quorum_key: P256Pair,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let turnkey_api_key =
            TurnkeyP256ApiKey::from_bytes(quorum_key.signing_key().to_bytes(), None)?;

        Ok(Self {
            ephemeral_key: Arc::new(ephemeral_key),
            quorum_key: Arc::new(quorum_key),
            turnkey_api_key: Arc::new(turnkey_api_key),
            http_client: HttpClient::new()?,
        })
    }
}
