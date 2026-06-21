//! Shared server state.

use qos_p256::P256Pair;
use std::sync::Arc;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) ephemeral_key: Arc<P256Pair>,
    pub(crate) quorum_key: Arc<P256Pair>,
}

impl AppState {
    /// Create a new application state value.
    #[must_use]
    pub fn from(ephemeral_key: P256Pair, quorum_key: P256Pair) -> Self {
        Self {
            ephemeral_key: Arc::new(ephemeral_key),
            quorum_key: Arc::new(quorum_key),
        }
    }
}
