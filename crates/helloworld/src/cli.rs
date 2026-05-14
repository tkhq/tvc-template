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
}
