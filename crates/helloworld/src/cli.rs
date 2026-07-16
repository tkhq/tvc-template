//! CLI argument parsing for the Hello World server
use clap::Parser;

/// Hello World REST server
#[derive(Parser, Debug)]
#[command(name = "helloworld", version, about = "Hello World REST server")]
pub struct Cli {
    /// IP address to listen on
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on
    #[arg(long, default_value = "44020")]
    pub port: u16,

    /// Path to the quorum key file
    #[arg(long, default_value = qos_core::QUORUM_FILE)]
    pub quorum_file: String,

    /// Path to the ephemeral key file used for app proofs
    #[arg(long, default_value = qos_core::EPHEMERAL_KEY_FILE)]
    pub ephemeral_file: String,

    /// Path to the manifest envelope file written by QOS at boot
    #[arg(long, default_value = qos_core::MANIFEST_FILE)]
    pub manifest_file: String,

    /// Use a mock NSM instead of the Nitro Secure Module device.
    /// DO NOT USE IN PRODUCTION: only for running outside an enclave.
    #[cfg(feature = "mock-nsm")]
    #[arg(long)]
    pub mock_nsm: bool,
}
