//! Shared server state.

use crate::client::HttpClient;
use qos_core::{
    EPHEMERAL_KEY_FILE, QUORUM_FILE,
    handles::{EphemeralKeyHandle, QuorumKeyHandle},
};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) ephemeral_key_handle: EphemeralKeyHandle<String>,
    pub(crate) quorum_key_handle: QuorumKeyHandle,
    pub(crate) http_client: HttpClient,
}

impl AppState {
    /// Create a new application state value.
    pub fn new(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self::new_with_http_client(
            ephemeral_key_handle,
            quorum_key_handle,
            HttpClient::new()?,
        ))
    }

    pub(crate) fn new_with_http_client(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
        http_client: HttpClient,
    ) -> Self {
        Self {
            ephemeral_key_handle,
            quorum_key_handle,
            http_client,
        }
    }
}

/// Create the default application state value.
pub fn default_app_state() -> Result<AppState, reqwest::Error> {
    AppState::new(
        EphemeralKeyHandle::new(EPHEMERAL_KEY_FILE.to_string()),
        QuorumKeyHandle::new(QUORUM_FILE.to_string()),
    )
}
