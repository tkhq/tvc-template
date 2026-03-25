---
name: tvc-demo-builder
description: "Builds TVC enclave demo applications on the tvc-template for Turnkey solutions engineering. Covers project setup, endpoint implementation, testing, container builds, and deployment. Use when asked to 'create a new TVC demo', 'build a demo for a client', 'add an endpoint to the demo', 'set up a new TVC app', 'scaffold a demo', 'implement a price oracle demo', 'build a settlement engine', 'create a policy signer demo', 'test the TVC app', 'run the demo', or 'deploy this to TVC'. Do NOT use for Turnkey wallet API operations (use managing-wallets-api), policy rule authoring (use managing-policies-api), or general Rust questions unrelated to TVC."
metadata:
  version: "1.0.0"
  author: turnkey
  tags: ["tvc", "demo", "solutions-engineering", "enclave", "workflow"]
---

# TVC Demo Builder

## Quick Start

Create a new TVC enclave demo by adding route handlers to `src/helloworld/src/router.rs`, writing tests, and building the OCI container.

## Prerequisites

- Rust 1.88+ (pinned in `src/rust-toolchain.toml`)
- Docker >= 26 with containerd snapshotter enabled (for OCI builds)
- The TVC CLI (`tvc`) for deployment (install from `github.com/tkhq/rust-sdk`)

## Phase 1: Understand the Template

Before writing code, read the existing structure. The template is a Rust workspace at `src/` with three crates:

| Crate | Path | Purpose |
|---|---|---|
| helloworld | `src/helloworld/` | Main REST server binary (your demo logic goes here) |
| metrics | `src/metrics/` | Prometheus metrics middleware (reuse as-is) |
| e2e | `src/e2e/` | Integration test harness (extend with your tests) |

Key files to read first:
- `src/helloworld/src/router.rs` for the route handler pattern
- `src/helloworld/src/main.rs` for server startup and middleware wiring
- `src/helloworld/src/cli.rs` for CLI argument structure
- `src/helloworld/Cargo.toml` for current dependencies

The workspace enforces strict safety at compile time: `unsafe` is forbidden, `unwrap()`/`expect()`/`panic!()` are denied in production code. Only test code may use `#[allow(...)]` to bypass these.

For complete architecture details, see [references/template-architecture.md](references/template-architecture.md).

## Phase 2: Plan the Demo

Before implementing, identify the demo's target audience and core value proposition:

### Audience Decision

- **Web3 / DeFi clients** (Polymarket, protocols): Emphasize verifiable computation, oracle integrity, fair ordering, tamper-proof settlement
- **TradFi / Banks** (JP Morgan, institutional): Emphasize compliance auditability, confidential computation, regulatory proof trails
- **Both**: Price oracles, policy-gated signers, sealed-bid auctions work across audiences

### Demo Architecture Pattern

Every TVC demo follows the same pattern:

```
Request -> Enclave receives input
        -> Enclave runs business logic (this is what you implement)
        -> Enclave produces signed/attested output
        -> Response includes App Proof for verification
```

Your job is implementing the business logic as Axum route handlers. The enclave infrastructure (attestation, signing, metrics, health checks) is provided by the template and TVC platform.

### Define Endpoints

Plan your endpoints before coding. Each demo typically needs:

1. A health check (already provided at `/health`)
2. One or more business logic endpoints (your demo)
3. Metrics (already provided at `/metrics`)

For concrete demo ideas with endpoint definitions, see [references/demo-examples.md](references/demo-examples.md).

## Phase 3: Implement the Demo

### Add Dependencies

If your demo needs new crates, add them to the workspace `src/Cargo.toml` under `[workspace.dependencies]`, then reference them in `src/helloworld/Cargo.toml`:

```toml
# In src/Cargo.toml (workspace root)
[workspace.dependencies]
reqwest = { version = "0.12", features = ["json"] }

# In src/helloworld/Cargo.toml
[dependencies]
reqwest = { workspace = true }
```

### Add Route Handlers

Add new handlers in `src/helloworld/src/router.rs`. Follow the existing pattern:

```rust
use axum::{
    Router,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::json;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/your_endpoint", post(your_handler))
        // ... existing routes
        .layer(TraceLayer::new_for_http())
}

async fn your_handler(
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl IntoResponse {
    // Your business logic here
    axum::Json(json!({"result": "computed_value"}))
}
```

### Error Handling

Since `unwrap()` and `expect()` are denied, use proper error handling:

```rust
async fn your_handler() -> Response {
    match some_fallible_operation() {
        Ok(result) => (StatusCode::OK, axum::Json(json!({"result": result}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"error": format!("{e}")})),
        ).into_response(),
    }
}
```

### For Complex Demos: Add a New Crate

If the demo logic is substantial, create a separate crate in the workspace instead of putting everything in `router.rs`:

```bash
cd src && cargo new --lib your-demo-logic
```

Then add it to `src/Cargo.toml`:

```toml
[workspace]
members = ["e2e", "metrics", "helloworld", "your-demo-logic"]
```

And depend on it from helloworld:

```toml
# In src/helloworld/Cargo.toml
[dependencies]
your-demo-logic = { path = "../your-demo-logic" }
```

## Phase 4: Test the Demo

### Run Existing Tests First

Before adding tests, verify the template's baseline passes:

```bash
make -C src test
```

### Write Unit Tests

Add unit tests inline in `router.rs` following the existing pattern. Unit tests use Tower's `oneshot()` to test handlers without starting a server:

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn body_string(body: Body) -> String {
        let bytes = body.collect().await.expect("failed to read body").to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
    }

    #[tokio::test]
    async fn test_your_endpoint() {
        let app = router();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/your_endpoint")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"input":"test"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert_eq!(json["result"], "expected_value");
    }
}
```

### Write E2E Tests

Add integration tests in `src/e2e/tests/`. These spawn the actual server binary and make real HTTP requests:

```rust
#![allow(missing_docs, clippy::unwrap_used)]
use e2e::TestArgs;

#[tokio::test]
async fn test_your_endpoint_e2e() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/your_endpoint", test_args.base_url))
            .json(&serde_json::json!({"input": "test"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["result"], "expected_value");
    }
    e2e::Builder::new().execute(test).await;
}
```

The `e2e::Builder` handles finding a free port, spawning the binary, waiting for readiness, and cleanup on drop.

### Run and Verify

```bash
# Run all tests
make -C src test

# Lint (catches clippy violations, missing docs, safety issues)
make -C src lint

# Run locally for manual testing
make -C src run
# Then: curl localhost:44020/your_endpoint
```

For detailed testing patterns and strategies, see [references/testing-guide.md](references/testing-guide.md).

## Phase 5: Build and Deploy

### Build the OCI Container

```bash
# Requires Docker >= 26 with containerd snapshotter
make out/helloworld/index.json
```

This produces a reproducible OCI image at `out/helloworld/`. The build:
1. Compiles a static musl binary (no dynamic linking)
2. Uses `SOURCE_DATE_EPOCH=1` for reproducibility
3. Runs compilation with `--network=none` (deps pre-fetched)
4. Produces identical binary hashes for identical source code

### Deploy to TVC

```bash
# Create the app (first time only)
tvc app create app-template.json

# Create a deployment
tvc deploy create deploy-template.json

# Approve the deployment (meets quorum threshold)
tvc deploy approve --deploy-id <DEPLOYMENT_UUID>
```

The deployment template specifies the container image URL (with SHA256 digest), the binary path (`/helloworld`), CLI args (host/port), and the expected binary digest for attestation verification.

### Compute the Binary Digest

For custom builds, extract and hash the binary to get the expected digest:

```bash
docker create --name tmp-extract <container_URL> /bin/true \
  && docker cp tmp-extract:/helloworld ./helloworld-binary \
  && docker rm tmp-extract
sha256sum ./helloworld-binary
```

This digest goes in the deployment template's `expectedPivotDigest` field.

## Rules

- Always run `make -C src test` and `make -C src lint` before committing changes
- Never use `unwrap()`, `expect()`, or `panic!()` in production code (compiler will reject it)
- Do not modify the `metrics` crate unless adding custom metric types
- Keep the `/health` endpoint as-is for orchestration compatibility
- Add new workspace dependencies to `src/Cargo.toml` first, then reference them with `workspace = true`
- For demos that fetch external data (oracles, APIs), handle network errors gracefully with proper `Result` types
- Container builds must be reproducible: do not add non-deterministic build steps
- E2E tests must use the `e2e::Builder` harness, not spawn servers manually
- When renaming the binary from "helloworld", update: `src/helloworld/Cargo.toml` (package name), `images/helloworld/Containerfile` (build paths), `Makefile` (build target), `src/e2e/src/lib.rs` (binary path), and `.github/workflows/` (CI references)

## Related Resources

- TVC Documentation: https://docs.turnkey.com/getting-started/verifiable-cloud-quickstart
- Secure Enclaves: https://docs.turnkey.com/security/secure-enclaves
- Turnkey Verified (App Proofs): https://docs.turnkey.com/security/turnkey-verified
- QuorumOS: https://docs.turnkey.com/security/quorum-deployments
- StageX (container builds): https://stagex.tools
- Axum framework: https://docs.rs/axum/latest/axum/
