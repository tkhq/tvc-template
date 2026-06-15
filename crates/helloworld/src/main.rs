//! Hello World REST server binary.

use clap::Parser;
use helloworld::cli::Cli;
use helloworld::router::{self, AppState};
use metrics::MetricsLayer;
use qos_core::handles::{EphemeralKeyHandle, QuorumKeyHandle};
use tracing_subscriber::EnvFilter;
use tvc_axum::ResponseSigningLayer;

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

    let ephemeral_key_handle = EphemeralKeyHandle::new(cli.ephemeral_file);
    let app_state = AppState::new(
        ephemeral_key_handle.clone(),
        QuorumKeyHandle::new(cli.quorum_file),
    );
    let app = router::router_with_state(app_state)
        .layer(metrics_layer)
        .route("/metrics", metrics::handler(collector))
        .layer(ResponseSigningLayer::new(ephemeral_key_handle));

    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
