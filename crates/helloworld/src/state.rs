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
    #[must_use]
    pub fn new(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
    ) -> Self {
        Self::new_with_http_client(ephemeral_key_handle, quorum_key_handle, HttpClient::new())
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

impl Default for AppState {
    fn default() -> Self {
        Self::new(
            EphemeralKeyHandle::new(EPHEMERAL_KEY_FILE.to_string()),
            QuorumKeyHandle::new(QUORUM_FILE.to_string()),
        )
    }
}
