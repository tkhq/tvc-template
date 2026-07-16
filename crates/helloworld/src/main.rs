//! Hello World REST server binary.

use clap::Parser;
use helloworld::cli::Cli;
use helloworld::router::{self, AppState};
use metrics::MetricsLayer;
use qos_core::protocol::services::boot::VersionedManifestEnvelope;
use qos_nsm::NsmProvider;
use qos_p256::P256Pair;
use std::io;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;
use tvc_axum::ResponseSigningLayer;

/// Return the NSM used to request attestation documents: the Nitro Secure
/// Module device, or a mock when running outside an enclave with
/// `--mock-nsm`.
#[cfg_attr(not(feature = "mock-nsm"), allow(unused_variables))]
fn nsm_provider(cli: &Cli) -> Arc<dyn NsmProvider + Send + Sync> {
    #[cfg(feature = "mock-nsm")]
    if cli.mock_nsm {
        return Arc::new(qos_nsm::mock::MockNsm::new());
    }
    Arc::new(qos_nsm::Nsm)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let metrics_layer = MetricsLayer::builder().namespace("tvc").build()?;
    let collector = metrics_layer.collector();

    let ephemeral_key = P256Pair::from_hex_file(&cli.ephemeral_file)
        .map_err(|e| io::Error::other(format!("failed to load ephemeral key: {e:?}")))?;
    let quorum_key = P256Pair::from_hex_file(&cli.quorum_file)
        .map_err(|e| io::Error::other(format!("failed to load quorum key: {e:?}")))?;
    let manifest_envelope = std::fs::read(&cli.manifest_file)
        .map_err(|e| io::Error::other(format!("failed to read manifest envelope: {e}")))
        .and_then(|bytes| {
            VersionedManifestEnvelope::try_from_slice_compat(&bytes)
                .map_err(|e| io::Error::other(format!("failed to decode manifest envelope: {e}")))
        })?;
    let nsm = nsm_provider(&cli);

    let app_state = AppState::new(ephemeral_key, quorum_key)?;
    let ephemeral_key = app_state.ephemeral_key();
    let quorum_key = app_state.quorum_key();
    let app = router::router_with_state(app_state)
        .layer(metrics_layer)
        .route("/metrics", metrics::handler(collector))
        .layer(
            ResponseSigningLayer::builder()
                .ephemeral_key(ephemeral_key)
                .quorum_key(quorum_key)
                .nsm(nsm)
                .manifest_envelope(manifest_envelope)
                .build()?,
        );

    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
