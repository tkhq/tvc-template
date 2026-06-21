//! Hello World REST server binary.

use clap::Parser;
use helloworld::cli::Cli;
use helloworld::router::{self, AppState};
use metrics::MetricsLayer;
use qos_p256::P256Pair;
use std::io;
use tracing_subscriber::EnvFilter;

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

    let ephemeral_key = P256Pair::from_hex_file(cli.ephemeral_file)
        .map_err(|e| io::Error::other(format!("failed to load ephemeral key: {e:?}")))?;
    let quorum_key = P256Pair::from_hex_file(cli.quorum_file)
        .map_err(|e| io::Error::other(format!("failed to load quorum key: {e:?}")))?;
    let app_state = AppState::from(ephemeral_key, quorum_key);
    let app = router::router_with_state(app_state)
        .layer(metrics_layer)
        .route("/metrics", metrics::handler(collector));

    let addr = format!("{}:{}", cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
