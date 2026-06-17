//! Hello World REST server binary.

use clap::Parser;
use helloworld::cli::Cli;
use helloworld::router::{self, AppState};
use metrics::MetricsLayer;
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

    let app_state = AppState::from_files(cli.ephemeral_file, cli.quorum_file)
        .map_err(|e| std::io::Error::other(format!("failed to load application keys: {e:?}")))?;
    let ephemeral_key = app_state.ephemeral_key();
    let quorum_key = app_state.quorum_key();
    let app = router::router_with_state(app_state)
        .layer(metrics_layer)
        .route("/metrics", metrics::handler(collector))
        .layer(
            ResponseSigningLayer::builder()
                .ephemeral_key(ephemeral_key)
                .quorum_key(quorum_key)
                .include_timestamp(true)
                .build(),
        );

    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
