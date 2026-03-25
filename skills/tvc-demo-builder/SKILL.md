---
name: tvc-demo-builder
description: "Builds and deploys TVC enclave applications on the tvc-template. Guides project setup, Rust endpoint implementation, testing, OCI container builds, and TVC deployment via Dashboard or CLI. Use when asked to 'create a TVC app', 'build a TVC application', 'add an endpoint', 'set up a new TVC project', 'scaffold a TVC service', 'implement an endpoint', 'test the TVC app', 'run the app locally', 'deploy to TVC', 'build the container', or 'how does the TVC template work'. Do NOT use for Turnkey wallet API operations (use managing-wallets-api), policy rule authoring (use managing-policies-api), or general Rust questions unrelated to TVC."
metadata:
  version: "1.0.0"
  author: turnkey
  tags: ["tvc", "enclave", "solutions-engineering", "workflow"]
---

# TVC App Builder

## Quick Start

Build a TVC enclave application by reading the current project structure, adding route handlers, writing tests, building the OCI container, and deploying via the TVC Dashboard or CLI.

## Prerequisites

- Rust toolchain (version pinned in `src/rust-toolchain.toml`)
- Docker >= 26 with containerd snapshotter enabled (for OCI builds)
- A Turnkey account with TVC access (app.turnkey.com/dashboard/tvc)
- Optionally, the TVC CLI (`tvc`) for CLI-based deployment (install from github.com/tkhq/rust-sdk)

## Understanding the Project

Before writing code, read the project to understand its current state. The template may have been modified since its initial creation. Always check:

1. **`src/Cargo.toml`** to see workspace members, dependencies, and lint rules
2. **The main binary crate's `router.rs`** (or equivalent) for the route handler pattern
3. **The main binary crate's `main.rs`** for server startup, middleware, and configuration
4. **The `Makefile` targets** for build, test, lint, and run commands
5. **`images/*/Containerfile`** for the container build pipeline

The workspace typically enforces strict safety at compile time: `unsafe` is forbidden, `unwrap()`/`expect()`/`panic!()` are denied in production code. Only test code may use `#[allow(...)]` to bypass these. Verify this by reading `[workspace.lints]` in `src/Cargo.toml`.

For a detailed walkthrough of the initial template architecture, see [references/template-architecture.md](references/template-architecture.md).

## Planning the Application

Before implementing, clarify the goals:

### What problem does the app solve?

TVC applications run inside AWS Nitro Enclaves with cryptographic attestation. The core value is provable, tamper-proof computation. Every TVC app follows this pattern:

```
Request -> Enclave receives input
        -> Enclave runs your business logic
        -> Enclave produces signed/attested output
        -> Response includes App Proof for verification
```

Ask: what computation needs to be verifiable? What trust assumption does this eliminate?

### Common application categories

- **Verifiable data processing**: Price oracles, benchmark calculators, data aggregation from multiple sources
- **Policy enforcement**: Transaction gates, compliance checks, spending limit enforcement, approval workflows
- **Confidential computation**: Multi-party risk aggregation, sealed-bid auctions, private data clean rooms
- **Fair ordering**: Anti-front-running services, verifiable sequencing, timestamp attestation
- **Settlement and resolution**: Deterministic outcome computation, market settlement, dispute resolution

For concrete endpoint designs across these categories, see [references/app-examples.md](references/app-examples.md).

### Define your endpoints

Plan endpoints before coding. Every app needs:
1. A health check (typically already provided at `/health`)
2. One or more business logic endpoints
3. Metrics (typically already provided at `/metrics`)

## Implementing the Application

### Adding dependencies

Add new crates to the workspace root `Cargo.toml` under `[workspace.dependencies]`, then reference them in the binary crate's `Cargo.toml`:

```toml
# In workspace Cargo.toml
[workspace.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }

# In your binary crate's Cargo.toml
[dependencies]
reqwest = { workspace = true }
```

Use `rustls-tls` instead of `native-tls` since the enclave has no system SSL libraries.

### Adding route handlers

Read the existing `router.rs` to understand the current pattern, then add your handlers following the same style. The typical Axum pattern:

```rust
async fn your_handler(
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> impl IntoResponse {
    // Your business logic
    axum::Json(json!({"result": "value"}))
}
```

Wire it into the router function:
```rust
.route("/your_endpoint", post(your_handler))
```

### Error handling

Since `unwrap()` and `expect()` are denied, always use proper error handling:

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

### Extracting complex logic into a separate crate

If the business logic is substantial, add a new library crate to the workspace rather than putting everything in the router file:

```bash
cd src && cargo new --lib your-logic
```

Add it to the workspace members in `src/Cargo.toml`, then depend on it from the binary crate.

### State management

Enclaves have no persistent storage. Use in-memory structures (`HashMap`, `Vec`, `RwLock`) for any state. Data resets on restart. For the application to be useful, either:
- Accept statelessness (each request is self-contained)
- Load state from configuration or external sources on startup
- Accept that in-memory state is ephemeral

## Testing the Application

### Verify the baseline first

```bash
make -C src test
make -C src lint
```

### Unit tests

Add inline tests in your router/handler files using Tower's `oneshot()` to test handlers without starting a server. Read the existing tests in the project and follow the same pattern. Key imports:

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    // ... tests using router().oneshot(request)
}
```

### E2E tests

If the project has an `e2e` crate, add integration tests that spawn the real server binary and make HTTP requests. Read the existing e2e test harness (`src/e2e/src/lib.rs`) to understand the `Builder` pattern, then follow it.

### Manual testing

```bash
make -C src run
# Then test with curl:
curl -s localhost:44020/health | jq
curl -s -X POST localhost:44020/your_endpoint -H "Content-Type: application/json" -d '{"key":"value"}' | jq
```

For detailed testing patterns, see [references/testing-guide.md](references/testing-guide.md).

## Building and Deploying

### Build the OCI container

```bash
make out/helloworld/index.json
```

This produces a reproducible OCI image. The build compiles a static musl binary with `SOURCE_DATE_EPOCH=1` and `--network=none` during compilation, ensuring identical binary hashes for identical source.

### Compute the binary digest

Extract and hash the binary to get the expected digest for deployment:

```bash
docker create --name tmp-extract <container_image_url> /bin/true \
  && docker cp tmp-extract:/<binary_name> ./binary \
  && docker rm tmp-extract
sha256sum ./binary
```

### Deploy via Dashboard (recommended for first-time setup)

1. **Create the app**: Go to app.turnkey.com/dashboard/tvc, click "Create app". Name it, paste your operator public key (from `tvc login`), click "Create new TVC App".

2. **Create a deployment**: Click into your app, click "Create deployment". Fill in:
   - **Container Image URL**: Your image URL with SHA256 digest (e.g., `ghcr.io/tkhq/helloworld@sha256:...`)
   - **Executable Path**: Path to binary in the container (e.g., `/helloworld`)
   - **Executable Args**: CLI arguments (e.g., `--host 0.0.0.0 --port 3000`)
   - **Public ingress port**: The port your app listens on (e.g., `3000`)
   - **Health check port**: Same port if health check is on the same server (e.g., `3000`)
   - **Health check type**: `HTTP`
   - **Executable digest**: The SHA256 hash of the binary file (from the step above)

3. **Approve the deployment**: Use the TVC CLI to approve:
   ```bash
   tvc deploy approve --deploy-id <DEPLOYMENT_UUID> --operator-id <OPERATOR_UUID>
   ```
   Find your operator ID under "Manifest Operators" in the app page on the dashboard.

4. **Access your app**: Once approved, the app is live at `https://app-<APP_UUID>.turnkey.cloud`

### Deploy via CLI

```bash
# Login (generates operator keypair, stored in ~/.config/turnkey/orgs/<name>/operator.json)
tvc login

# Create app template and create the app
tvc app init --output my-app.json
# Edit my-app.json: set name, verify operator key
tvc app create my-app.json

# Create deployment template and deploy
tvc deploy init
# Edit the generated JSON: set appId, qosVersion, pivotContainerImageUrl,
# pivotPath, pivotArgs, expectedPivotDigest, publicIngressPort,
# healthCheckPort, healthCheckType
tvc deploy create deploy.json

# Approve (meets manifest set threshold)
tvc deploy approve --deploy-id <DEPLOYMENT_UUID> --operator-id <OPERATOR_UUID>
```

### Deployment configuration fields

| Field | Description | Example |
|---|---|---|
| `pivotContainerImageUrl` | OCI image URL with SHA256 digest | `ghcr.io/tkhq/helloworld@sha256:f813...` |
| `pivotPath` | Path to binary in the container | `/helloworld` |
| `pivotArgs` | CLI arguments passed to the binary | `--host 0.0.0.0 --port 3000` |
| `expectedPivotDigest` | SHA256 hash of the binary file | `cbe011...` |
| `publicIngressPort` | Port exposed to the internet | `3000` |
| `healthCheckPort` | Port for health checks | `3000` |
| `healthCheckType` | Health check protocol | `HTTP` or `TVC_HEALTH_CHECK_TYPE_HTTP` |
| `qosVersion` | QuorumOS version | `v2026.2.6` |
| `pivotContainerEncryptedPullSecret` | Encrypted pull secret for private images | (optional) |

## Rules

- Always run tests and lint before committing
- Never use `unwrap()`, `expect()`, or `panic!()` in production code (compiler rejects it)
- Do not modify shared middleware crates (metrics) unless adding custom metric types
- Keep the `/health` endpoint functional for orchestration and TVC health checks
- Add workspace dependencies at the workspace level first, then reference with `workspace = true`
- Handle network errors gracefully for any external API calls
- Container builds must be reproducible (no non-deterministic build steps)
- When renaming the binary, update all references: Cargo.toml, Containerfile, Makefile, e2e test harness, and CI workflows
- Use `rustls-tls` for HTTP clients (no system SSL in enclaves)

## Related Resources

- TVC Quickstart: https://docs.turnkey.com/getting-started/verifiable-cloud-quickstart
- TVC Dashboard: https://app.turnkey.com/dashboard/tvc
- Secure Enclaves: https://docs.turnkey.com/security/secure-enclaves
- Turnkey Verified (App Proofs): https://docs.turnkey.com/security/turnkey-verified
- QuorumOS: https://docs.turnkey.com/security/quorum-deployments
- StageX (container builds): https://stagex.tools
- Axum framework: https://docs.rs/axum/latest/axum/
