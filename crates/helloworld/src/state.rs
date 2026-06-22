//! Shared server state.

use qos_core::{
    EPHEMERAL_KEY_FILE, MANIFEST_FILE, QUORUM_FILE,
    handles::{EphemeralKeyHandle, QuorumKeyHandle},
};
use qos_nsm::NsmProvider;
use std::sync::Arc;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) ephemeral_key_handle: EphemeralKeyHandle<String>,
    pub(crate) quorum_key_handle: QuorumKeyHandle,
    pub(crate) manifest_file: String,
    pub(crate) nsm_provider: Arc<dyn NsmProvider>,
}

impl AppState {
    /// Create a new application state value.
    #[must_use]
    pub fn new(
        ephemeral_key_handle: EphemeralKeyHandle<String>,
        quorum_key_handle: QuorumKeyHandle,
        manifest_file: String,
        nsm_provider: Arc<dyn NsmProvider>,
    ) -> Self {
        Self {
            ephemeral_key_handle,
            quorum_key_handle,
            manifest_file,
            nsm_provider,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new(
            EphemeralKeyHandle::new(EPHEMERAL_KEY_FILE.to_string()),
            QuorumKeyHandle::new(QUORUM_FILE.to_string()),
            MANIFEST_FILE.to_string(),
            Arc::new(qos_nsm::Nsm),
        )
    }
}
