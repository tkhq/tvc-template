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

use std::future::Future;
use std::net::TcpListener;
use std::ops::Range;
use std::process::Command;
use std::thread;
use std::time::Duration;

const MAX_PORT_BIND_WAIT_TIME: Duration = Duration::from_secs(90);
const PORT_BIND_WAIT_TIME_INCREMENT: Duration = Duration::from_millis(500);
const POST_BIND_SLEEP: Duration = Duration::from_millis(500);
const SERVER_PORT_RANGE: Range<u16> = 10000..60000;
const MAX_PORT_SEARCH_ATTEMPTS: u16 = 50;

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
    for _ in 0..MAX_PORT_SEARCH_ATTEMPTS {
        let port = rand::random_range(SERVER_PORT_RANGE);
        if port_is_available(port) {
            return Some(port);
        }
    }

    None
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

const HOST_IP: &str = "127.0.0.1";

/// Arguments passed to the `test` function in [`Builder::execute`].
pub struct TestArgs {
    /// The base URL for the REST server (e.g. `http://127.0.0.1:12345`)
    pub base_url: String,
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
    /// Note this test env builder relies on the `helloworld` binary already
    /// being built and existing in the target directory. Run `cargo build`
    /// from the workspace root before running integration tests.
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

        let server_binary =
            workspace_binary("helloworld").expect("failed to locate helloworld binary");

        let _server_process: ChildWrapper = Command::new(server_binary)
            .arg("--host")
            .arg(HOST_IP)
            .arg("--port")
            .arg(host_port.to_string())
            .spawn()
            .expect("failed to spawn helloworld binary")
            .into();

        wait_until_port_is_bound(host_port);

        let base_url = format!("http://{HOST_IP}:{host_port}");

        let test_args = TestArgs { base_url };

        test(test_args).await;
    }
}

fn workspace_binary(name: &str) -> std::io::Result<std::path::PathBuf> {
    let mut path = std::env::current_exe()?;
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push(name);
    Ok(path)
}
