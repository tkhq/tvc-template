//! Utilities for integration tests
#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::struct_excessive_bools,
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use qos_p256::{P256Pair, P256Public};
use std::future::Future;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

const MAX_PORT_BIND_WAIT_TIME: Duration = Duration::from_secs(90);
const PORT_BIND_WAIT_TIME_INCREMENT: Duration = Duration::from_millis(500);
const POST_BIND_SLEEP: Duration = Duration::from_millis(500);

/// Wrapper type for [`std::process::Child`] that kills the process on drop.
#[derive(Debug)]
pub struct ChildWrapper(std::process::Child);

impl From<std::process::Child> for ChildWrapper {
    fn from(child: std::process::Child) -> Self {
        Self(child)
    }
}

impl Drop for ChildWrapper {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

/// Get a bind-able TCP port on the local system.
#[must_use]
pub fn find_free_port() -> Option<u16> {
    match TcpListener::bind((HOST_IP, 0)) {
        Ok(listener) => listener.local_addr().ok().map(|addr| addr.port()),
        Err(error) => panic!("failed to bind an OS-assigned local port: {error}"),
    }
}

/// Wait until the given `port` is bound. Helpful for telling if something is
/// listening on the given port.
///
/// # Panics
///
/// Panics if the the port is not bound to within `MAX_PORT_BIND_WAIT_TIME`.
pub fn wait_until_port_is_bound(port: u16) {
    let mut wait_time = PORT_BIND_WAIT_TIME_INCREMENT;

    while wait_time < MAX_PORT_BIND_WAIT_TIME {
        thread::sleep(wait_time);
        if port_is_available(port) {
            wait_time += PORT_BIND_WAIT_TIME_INCREMENT;
        } else {
            thread::sleep(POST_BIND_SLEEP);
            return;
        }
    }
    panic!(
        "Server has not come up: port {} is still available after {}s",
        port,
        MAX_PORT_BIND_WAIT_TIME.as_secs()
    )
}

/// Return whether or not the port can be bound to.
fn port_is_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn helloworld_binary() -> PathBuf {
    static SERVER_BINARY: OnceLock<PathBuf> = OnceLock::new();

    SERVER_BINARY
        .get_or_init(|| {
            if let Some(binary) = std::env::var_os("CARGO_BIN_EXE_helloworld") {
                return PathBuf::from(binary);
            }

            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let status = Command::new(cargo)
                .args(["build", "-p", "helloworld"])
                .status()
                .expect("failed to build helloworld binary");
            assert!(status.success(), "failed to build helloworld binary");

            let mut binary = std::env::current_exe().expect("failed to get current test binary");
            binary.pop();
            if binary.ends_with("deps") {
                binary.pop();
            }
            binary.push(format!("helloworld{}", std::env::consts::EXE_SUFFIX));
            binary
        })
        .clone()
}

const HOST_IP: &str = "127.0.0.1";

/// Arguments passed to the `test` function in [`Builder::execute`].
pub struct TestArgs {
    /// The base URL for the REST server (e.g. `http://127.0.0.1:12345`)
    pub base_url: String,
    /// Public key corresponding to the ephemeral response-signing key.
    pub ephemeral_public_key: P256Public,
    /// Public key corresponding to the quorum response-signing key.
    pub quorum_public_key: P256Public,
}

/// Test harness builder.
#[derive(Default)]
pub struct Builder {}

impl Builder {
    /// Create a new instance of [`Self`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Execute `test`.
    ///
    /// Spawns the `helloworld` binary, waits for it to bind, then runs
    /// the provided test function with a [`TestArgs`] containing the base URL.
    ///
    /// # Panics
    ///
    /// Panics if `test` panics or the server binary cannot be spawned.
    pub async fn execute<F, T>(self, test: F)
    where
        F: Fn(TestArgs) -> T,
        T: Future<Output = ()>,
    {
        let host_port =
            find_free_port().expect("failed to find a free port after maximum search attempts");

        let server_binary = helloworld_binary();
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let ephemeral_key_path = temp_dir.path().join("qos.ephemeral.key");
        let quorum_key_path = temp_dir.path().join("qos.quorum.key");
        let ephemeral_key = P256Pair::generate().expect("failed to generate ephemeral key");
        let quorum_key = P256Pair::generate().expect("failed to generate quorum key");
        let ephemeral_public_key = P256Public::from_bytes(&ephemeral_key.public_key().to_bytes())
            .expect("failed to decode ephemeral public key");
        let quorum_public_key = P256Public::from_bytes(&quorum_key.public_key().to_bytes())
            .expect("failed to decode quorum public key");
        ephemeral_key
            .to_hex_file(&ephemeral_key_path)
            .expect("failed to write ephemeral key");
        quorum_key
            .to_hex_file(&quorum_key_path)
            .expect("failed to write quorum key");

        let _server_process: ChildWrapper = Command::new(server_binary)
            .arg("--host")
            .arg(HOST_IP)
            .arg("--port")
            .arg(host_port.to_string())
            .arg("--ephemeral-file")
            .arg(&ephemeral_key_path)
            .arg("--quorum-file")
            .arg(&quorum_key_path)
            .spawn()
            .expect("failed to spawn helloworld binary")
            .into();

        wait_until_port_is_bound(host_port);

        let base_url = format!("http://{HOST_IP}:{host_port}");

        let test_args = TestArgs {
            base_url,
            ephemeral_public_key,
            quorum_public_key,
        };

        test(test_args).await;
    }
}
