//! Reusable application startup wiring.

use crate::{router, state::AppState};
use axum::Router;
use metrics::MetricsLayer;
use qos_core::handles::{EphemeralKeyHandle, QuorumKeyHandle};
use qos_nsm::NsmProvider;
use std::{error::Error, net::SocketAddr, sync::Arc};

/// Runtime arguments needed to build and run the app.
pub struct AppArgs {
    /// Address the HTTP server binds to.
    pub bind_addr: SocketAddr,
    /// Path to the app ephemeral key file.
    pub ephemeral_file: String,
    /// Path to the quorum key file.
    pub quorum_file: String,
    /// Path to the QOS manifest envelope file.
    pub manifest_file: String,
    /// NSM provider used to generate attestation documents.
    pub nsm_provider: Arc<dyn NsmProvider>,
}

/// Build the application router from runtime arguments.
///
/// # Errors
///
/// Returns an error if metrics initialization fails.
pub fn build_router(args: AppArgs) -> Result<Router, Box<dyn Error>> {
    let metrics_layer = MetricsLayer::builder().namespace("tvc").build()?;
    let collector = metrics_layer.collector();
    let app_state = AppState::new(
        EphemeralKeyHandle::new(args.ephemeral_file),
        QuorumKeyHandle::new(args.quorum_file),
        args.manifest_file,
        args.nsm_provider,
    );

    Ok(router::router_with_state(app_state)
        .layer(metrics_layer)
        .route("/metrics", metrics::handler(collector)))
}

/// Run the HTTP server until shutdown.
///
/// # Errors
///
/// Returns an error if the listener cannot bind, metrics cannot initialize, or
/// the HTTP server exits with an error.
pub async fn run(args: AppArgs) -> Result<(), Box<dyn Error>> {
    let bind_addr = args.bind_addr;
    let app = build_router(args)?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("Server listening on {bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
