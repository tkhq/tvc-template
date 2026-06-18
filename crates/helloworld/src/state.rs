//! Shared server state.

use qos_core::{EPHEMERAL_KEY_FILE, QUORUM_FILE};
use qos_p256::{P256Error, P256Pair};
use std::{path::Path, sync::Arc};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub(crate) ephemeral_key: Arc<P256Pair>,
    pub(crate) quorum_key: Arc<P256Pair>,
}

impl AppState {
    /// Create a new application state value.
    #[must_use]
    pub fn new(ephemeral_key: Arc<P256Pair>, quorum_key: Arc<P256Pair>) -> Self {
        Self {
            ephemeral_key,
            quorum_key,
        }
    }

    /// Create application state by loading key files once.
    ///
    /// # Errors
    ///
    /// Returns an error if either key file cannot be decoded as a P-256 key pair.
    pub fn from_key_files<P, Q>(
        ephemeral_key_file: P,
        quorum_key_file: Q,
    ) -> Result<Self, P256Error>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        let ephemeral_key = P256Pair::from_hex_file(ephemeral_key_file)?;
        let quorum_key = P256Pair::from_hex_file(quorum_key_file)?;

        Ok(Self::new(Arc::new(ephemeral_key), Arc::new(quorum_key)))
    }

    /// Create application state from the default QOS key paths.
    ///
    /// # Errors
    ///
    /// Returns an error if either default key file cannot be decoded as a P-256 key pair.
    pub fn try_default() -> Result<Self, P256Error> {
        Self::from_key_files(EPHEMERAL_KEY_FILE, QUORUM_FILE)
    }
}
