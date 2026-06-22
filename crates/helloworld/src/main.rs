//! Hello World REST server binary.

use clap::Parser;
use helloworld::{app, cli::Cli};
use qos_nsm::NsmProvider;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let bind_addr = format!("{}:{}", cli.host, cli.port).parse()?;
    let nsm_provider: Arc<dyn NsmProvider> = if cli.unsafe_mock_nsm {
        Arc::new(qos_nsm::mock::DynamicMockNsm::new().with_mock_certificate_chain())
    } else {
        Arc::new(qos_nsm::Nsm)
    };

    app::run(app::AppArgs {
        bind_addr,
        ephemeral_file: cli.ephemeral_file,
        quorum_file: cli.quorum_file,
        manifest_file: cli.manifest_file,
        nsm_provider,
    })
    .await?;
    Ok(())
}
