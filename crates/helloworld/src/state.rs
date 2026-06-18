//! Shared server state.

use qos_core::{
    EPHEMERAL_KEY_FILE, QUORUM_FILE,
    handles::{EphemeralKeyHandle, QuorumKeyHandle},
};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) ephemeral_key_handle: EphemeralKeyHandle<String>,
    pub(crate) quorum_key_handle: QuorumKeyHandle,
}

impl AppState {
    /// Create a new application state value.
    #[must_use]
    pub fn new(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
    ) -> Self {
        Self {
            ephemeral_key_handle,
            quorum_key_handle,
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
