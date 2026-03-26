# Template Architecture Reference

## Project Layout

```
tvc-template/
  src/
    Cargo.toml              # Workspace root: defines crates, shared deps, lint rules
    Makefile                 # Dev commands: build, test, lint, run
    rust-toolchain.toml      # Pins Rust 1.88 with clippy + rustfmt
    helloworld/              # Main application crate
      Cargo.toml             # Binary dependencies
      src/
        main.rs              # Entry point: tracing, metrics, server startup
        router.rs            # HTTP route handlers + unit tests (YOUR CODE GOES HERE)
        cli.rs               # CLI args via clap (--host, --port)
        lib.rs               # Library re-exports
    metrics/                 # Reusable Prometheus middleware (do not modify)
      Cargo.toml
      src/
        lib.rs               # Public API: MetricsLayer, MetricsCollector, handler
        layer.rs             # Tower Layer + Service impl (records request duration)
        handler.rs           # GET /metrics endpoint (Prometheus text format)
    e2e/                     # Integration test harness
      Cargo.toml
      src/
        lib.rs               # Builder, TestArgs, find_free_port, wait_until_port_is_bound
      tests/
        helloworld.rs        # E2E tests for all endpoints
  images/
    helloworld/
      Containerfile          # Two-stage StageX build (build + package)
  Makefile                   # Top-level: OCI container build
  .github/
    workflows/
      main.yml               # CI: lint + test on push/PR
      stagex.yml             # Container build + push to ghcr.io
    actions/
      docker-setup/          # Custom action: containerd, registry mirrors, auth
```

## Server Startup Flow (main.rs)

```
1. Initialize tracing (structured logging via tracing-subscriber)
   - Reads RUST_LOG env var, defaults to "info"

2. Parse CLI args (clap)
   - --host (default: 127.0.0.1)
   - --port (default: 44020)

3. Build MetricsLayer
   - Creates Prometheus histogram with "tvc" namespace
   - Records: method, path, status code, duration in ms

4. Build Axum Router
   - router() from router.rs (your business logic routes)
   - .layer(metrics_layer) wraps all routes with metrics
   - .route("/metrics", handler) adds the Prometheus endpoint

5. Bind TcpListener and serve
```

## Middleware Stack (applied to every request)

```
Request
  -> TraceLayer (logs method, URI, status, latency)
    -> MetricsLayer (records histogram: tvc_http_request_duration_ms)
      -> Your Route Handler
    <- MetricsLayer (records response status + duration)
  <- TraceLayer (logs completion)
Response
```

The `/metrics` endpoint is added AFTER the metrics layer, so it does not record its own requests in the histogram.

## Workspace Lint Rules (src/Cargo.toml)

These are enforced at compile time across all crates:

```toml
[workspace.lints.clippy]
unwrap_used = "deny"    # No .unwrap() in production code
expect_used = "deny"    # No .expect() in production code
panic = "deny"          # No panic!() in production code

[workspace.lints.rust]
unsafe_code = "forbid"  # No unsafe blocks anywhere
missing_docs = "warn"   # Doc comments encouraged
```

Test code bypasses these with: `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`

## Container Build Pipeline (images/helloworld/Containerfile)

**Stage 1: Build**
- Base: `stagex/pallet-rust:1.88.0` (deterministic Rust toolchain)
- Flags: `--target x86_64-unknown-linux-musl --release` (static binary)
- `RUSTFLAGS='-C target-feature=+crt-static'` (link C runtime statically)
- `cargo fetch` first (cache deps), then build with `--network=none` (reproducible)
- Output: single static binary at `/rootfs/helloworld`

**Stage 2: Package**
- Base: `stagex/core-busybox:1.36.1` (minimal filesystem)
- Copies only the binary from build stage
- Final image: ~10MB, single binary, no libc dependency

**Reproducibility guarantees:**
- `SOURCE_DATE_EPOCH=1` in the Makefile (deterministic timestamps)
- `--network=none` during build (no external fetches)
- Pinned base images with SHA256 digests (no tag drift)
- Same source code always produces the same binary hash

## E2E Test Harness (src/e2e/src/lib.rs)

The `Builder` pattern:

```rust
// Usage in tests:
e2e::Builder::new().execute(|test_args| async move {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/health", test_args.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}).await;
```

What `Builder::execute` does:
1. Finds a free port (random in range 10000-60000, tries up to 50 times)
2. Spawns `../target/debug/helloworld --host 127.0.0.1 --port <port>`
3. Waits up to 90s for the port to be bound (polls every 500ms)
4. Passes `TestArgs { base_url: "http://127.0.0.1:<port>" }` to your test function
5. On drop (test completion or panic), kills the server process

The binary must be pre-built. `make -C src test` runs `cargo build --all` first via the `build` dependency in the Makefile.

## CI/CD Workflows

### main.yml (Lint + Test)
- Triggers: push to main, PRs to main
- Steps: install Rust 1.88, cargo clippy, cargo test

### stagex.yml (Container Build + Push)
- Triggers: tags (v*.*.*), push to main, PRs, manual dispatch
- Steps: docker setup, build OCI image, push to ghcr.io/tkhq/helloworld
- Tags: `latest` (main), `pr-N` (PRs), version tags (releases)
