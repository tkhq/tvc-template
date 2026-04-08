---
name: tvc-app-builder
description: "Builds and deploys TVC (Turnkey Verifiable Cloud) enclave applications on the tvc-template. Covers Rust endpoints, Axum route handlers, testing, OCI container builds, TVC CLI deployment, and calling the Turnkey API from TVC apps. Use when asked to create/build/deploy a TVC app, add TVC endpoints or route handlers, write TVC tests, run tvc login/deploy/approve, fix TVC deployment 404, set up TVC CI/CD, build TVC containers, build sealed-bid auctions or settlement engines or prediction markets on TVC, call Turnkey API from TVC, or stamp Turnkey requests in Rust for TVC. Do NOT use for signing transactions via Turnkey API (use signing-transactions-api), creating wallets (use managing-wallets-api), managing policies (use managing-policies-api), standalone Turnkey API auth (use stamping-api), or general Rust unrelated to TVC."
metadata:
  version: "3.3.0"
  author: turnkey
  tags: ["tvc", "enclave", "solutions-engineering", "workflow", "deployment"]
---

# TVC App Builder

## Quick Start

Build a TVC enclave application by reading the current project structure, adding route handlers, writing tests, building the OCI container, and deploying autonomously via the TVC CLI.

## Prerequisites

- Rust toolchain (version pinned in `src/rust-toolchain.toml`)
- Docker with buildx plugin (`brew install docker-buildx`, then add `"cliPluginsExtraDirs": ["/opt/homebrew/lib/docker/cli-plugins"]` to `~/.docker/config.json`)
- The TVC CLI (`tvc`), installed from [github.com/tkhq/rust-sdk](https://github.com/tkhq/rust-sdk): `git clone git@github.com:tkhq/rust-sdk.git && cd rust-sdk && cargo install --path tvc`
- A container registry account (ghcr.io recommended). Container images MUST be public or deployed with a pull secret.
- `jq` for parsing JSON CLI output
- `gh` CLI for GitHub authentication (used to obtain ghcr.io login tokens)

## Understanding the Project

Before writing code, read the project to understand its current state. The template may have been modified since its initial creation. Always check:

1. **`src/Cargo.toml`** to see workspace members, dependencies, and lint rules
2. **The main binary crate's `router.rs`** (or equivalent) for the route handler pattern
3. **The main binary crate's `main.rs`** for server startup, middleware, and configuration
4. **The `Makefile` targets** for build, test, lint, and run commands
5. **`images/*/Containerfile`** for the container build pipeline

The workspace enforces strict safety at compile time: `unsafe` is forbidden, `unwrap()`/`expect()`/`panic!()` are denied in production code. Only test code may use `#[allow(...)]` to bypass these. Verify this by reading `[workspace.lints]` in `src/Cargo.toml`.

For a detailed walkthrough of the initial template architecture, see [references/template-architecture.md](references/template-architecture.md).

## Designing for Enclaves

TVC runs multiple enclave instances behind a load balancer. Each instance has isolated, independent memory. This has critical design implications:

**Design for stateless, deterministic computation.** The ideal TVC app pattern is:

```
Input -> Deterministic logic -> Attested output
```

The enclave's value is proving *what computation happened*, not storing results. The App Proof attached to the response is the verification mechanism.

**If you use in-memory state** (`Arc<RwLock<...>>`), understand that:
- Different requests may hit different enclave instances
- Each instance maintains its own independent state
- Sequential IDs, counters, and lookups are only consistent within a single instance
- A POST that writes state and a subsequent GET may hit different instances

**Recommended patterns:**
- Stateless request/response (each request is self-contained)
- Client stores results, not the enclave (return signed/attested data the client can verify later)
- If state is needed, accept it as ephemeral per-instance

For concrete endpoint designs, see [references/app-examples.md](references/app-examples.md). For patterns on calling the Turnkey API from TVC apps or deployment scripts (user management, stamping, key generation), see [references/turnkey-api-integration.md](references/turnkey-api-integration.md).

## Planning the Application

### What problem does the app solve?

TVC applications run inside AWS Nitro Enclaves with cryptographic attestation. The core value is provable, tamper-proof computation. Ask: what computation needs to be verifiable? What trust assumption does this eliminate?

### Common application categories

- **Verifiable data processing**: Price oracles, benchmark calculators, data aggregation
- **Policy enforcement**: Transaction gates, compliance checks, spending limits
- **Confidential computation**: Sealed-bid auctions, private data clean rooms
- **Fair ordering**: Anti-front-running services, verifiable sequencing
- **Settlement and resolution**: Deterministic outcome computation, market settlement

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

### State management

If shared mutable state is needed, use `Arc<RwLock<YourState>>` and pass it to the router via Axum's state mechanism. See the "Designing for Enclaves" section above for important caveats.

## Testing the Application

### Verify the baseline first

```bash
make -C src test
make -C src lint
```

### Unit tests

Add inline tests using Tower's `oneshot()`. Key imports:

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
}
```

### E2E tests

If the project has an `e2e` crate, add integration tests that spawn the real server binary. Read the existing `Builder` pattern in `src/e2e/src/lib.rs`.

For detailed testing patterns, see [references/testing-guide.md](references/testing-guide.md).

## Security: Agent-Assisted Deployment

Deploying to TVC from an AI agent or automated pipeline requires extra caution. Enclave deployments are difficult to reverse and directly affect production infrastructure.

**Mandatory confirmation gates (agents MUST follow these):**
- ALWAYS confirm with the user before running `tvc deploy approve --yes`. This command cryptographically approves a deployment manifest, which triggers enclave provisioning. Once approved, the deployment goes live.
- ALWAYS confirm with the user before running `tvc deploy create`. This creates a deployment that consumes infrastructure resources.
- ALWAYS show the user the full deploy.json and app.json contents before creating resources, so they can review the configuration.
- NEVER run `tvc deploy approve` with `--yes` in production environments without explicit user authorization for that specific deployment.

**Credential handling:**
- NEVER commit API key files, operator key files, or pull secrets to version control.
- NEVER log or display private key material. The CLI stores keys at `~/.config/turnkey/orgs/<alias>/` and these paths should not be read or echoed.
- When using `--api-key-file`, pass the path to the file, not the file contents.
- API keys and operator keys are P256 keypairs with full signing authority over the organization. Treat them like production database credentials.
- Prefer scoped API keys with minimal permissions when possible.

**Deployment safety:**
- Use `tvc deploy approve --dry-run` first to review the manifest before generating an actual approval.
- Use `tvc deploy approve --skip-post` to generate the approval locally without posting it, allowing manual review before submission.
- Always verify `tvc deploy status` returns "Live" and confirm the app responds on its URL before considering a deployment complete.
- Apps cannot currently be deleted through the CLI or dashboard. Create app names carefully.

## Building and Deploying (Autonomous CLI Workflow)

The TVC CLI (from [github.com/tkhq/rust-sdk](https://github.com/tkhq/rust-sdk)) supports fully non-interactive operation using `--json`, `--no-input`, and `--yes` flags. All flags have corresponding `TVC_*` environment variables. For the complete CLI reference, see [references/tvc-cli-guide.md](references/tvc-cli-guide.md).

### Step 1: Build and test

```bash
make -C src test
make -C src lint
```

### Step 2: Build the OCI container

```bash
make out/<binary_name>/index.json
```

### Step 3: Push to a PUBLIC container registry

TVC infrastructure must be able to pull the container image. Images on ghcr.io are private by default. You MUST make the package public in GitHub package settings before the enclave can pull it, or provide a pull secret during deployment.

**Push steps:**
```bash
# Authenticate to ghcr.io (use gh CLI token)
gh auth token | docker login ghcr.io --username <github_user> --password-stdin

# Load and push
docker load -i <(tar -cf - -C out/<binary_name> .)
docker tag <local_tag> ghcr.io/<user>/<repo>:latest
docker push ghcr.io/<user>/<repo>:latest
# Capture the digest from push output (sha256:...)
```

**Make the package public (required unless using pull secret):**
Go to `https://github.com/users/<USERNAME>/packages/container/<PACKAGE>/settings`, scroll to "Danger Zone", click "Change visibility", select "Public". This cannot be done programmatically for user-scoped packages.

**Verify an image is publicly pullable:**
```bash
TOKEN=$(curl -s "https://ghcr.io/token?scope=repository:USER/REPO:pull" | jq -r '.token')
curl -s -o /dev/null -w "%{http_code}" \
  "https://ghcr.io/v2/USER/REPO/manifests/sha256:DIGEST" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Accept: application/vnd.oci.image.manifest.v1+json"
# Must return 200. If 401, the image is still private.
```

### Step 4: Compute the binary digest

This is the SHA256 hash of the binary file inside the container (not the container image digest).

```bash
docker create --name tmp-extract ghcr.io/<user>/<repo>@sha256:<image_digest> /bin/true \
  && docker cp tmp-extract:/<binary_name> ./binary \
  && docker rm tmp-extract
sha256sum ./binary
# Use this hash as expectedPivotDigest
```

### Step 5: Login (if not already authenticated)

There are two authentication paths:

**Path A: Import an existing API key from the Turnkey dashboard (recommended for agents)**
```bash
# Download API credentials JSON from the Turnkey dashboard, then:
tvc login --org-id <ORG_UUID> --api-key-file /path/to/credentials.json --api-env prod --no-input
```
The CLI accepts both the dashboard export format (JSON array with `apiKeyName`, `publicKey`, `privateKey`, `curveType`) and the CLI internal format. The key is imported, converted, and verified in one step.

**Path B: Let the CLI generate a new key**
```bash
tvc login --org-id <ORG_UUID> --api-env prod --no-input
```
The CLI generates a P256 keypair and prints the public key. You must add this public key to your organization in the Turnkey dashboard before the key will work. Run `tvc login --org default` again after adding the key to verify.

**Path C: Bypass login entirely with global override flags**
```bash
export TVC_API_KEY_FILE=/path/to/api_key.json
export TVC_API_URL=https://api.turnkey.com
export TVC_ORG_ID=<your-org-uuid>
```

### Step 6: Create app, deploy, and approve (fully autonomous)

```bash
# Create the app config
tvc app init --output app.json
# Fill in: name and manifestSetParams.name
# The operator and quorum keys are auto-populated from login
# IMPORTANT: Set "externalConnectivity": true if the app needs to be reachable
# from the internet. The default is false, which means the ingress proxy will
# return 404 for all requests even though the enclave is running.

# Create the app and capture IDs
APP_RESULT=$(tvc --json app create app.json)
APP_ID=$(echo "$APP_RESULT" | jq -r '.app_id')
OPERATOR_ID=$(echo "$APP_RESULT" | jq -r '.manifest_set_operator_ids[0]')

# Create the deployment config (appId is auto-filled from last created app)
tvc deploy init --output deploy.json
# Fill in: pivotContainerImageUrl (with @sha256:), pivotPath, pivotArgs,
# expectedPivotDigest, publicIngressPort (3000), healthCheckPort (3000),
# healthCheckType (TVC_HEALTH_CHECK_TYPE_HTTP), qosVersion (v2026.2.6)
# REMOVE pivotContainerEncryptedPullSecret field entirely if image is public

# Create the deployment and capture ID
DEPLOY_RESULT=$(tvc --json deploy create deploy.json)
DEPLOY_ID=$(echo "$DEPLOY_RESULT" | jq -r '.deployment_id')

# Approve non-interactively
tvc --json --no-input deploy approve \
  --deploy-id "$DEPLOY_ID" \
  --operator-id "$OPERATOR_ID" \
  --yes

# Check status (wait 1-2 minutes after approval for enclave provisioning)
tvc --json deploy status --deploy-id "$DEPLOY_ID"
```

### Step 7: Access the deployed app

The app URL depends on the API environment you authenticated against:

| Environment | App URL Pattern |
|---|---|
| Production | `https://app-<APP_UUID>.turnkey.cloud` |
| Dev | `https://app-<APP_UUID>.tvc.dev.turnkey.engineering` |

Check `api_base_url` in `~/.config/turnkey/tvc.config.toml` to determine which environment you are using. TVC automatically provisions TLS and network ingress. The enclave may take 1-2 minutes after approval before it begins responding (a `404 page not found` from the ingress proxy during this window is normal).

### App configuration fields

| Field | Description | Example |
|---|---|---|
| `name` | App name (must be unique in the org, cannot be deleted) | `my-app` |
| `externalConnectivity` | Whether the app is reachable from the internet. **Set to `true` for any app that serves HTTP traffic.** Default is `false`. | `true` |
| `manifestSetParams.name` | Name for the manifest set | `my-manifest-set` |
| `manifestSetParams.threshold` | Number of operator approvals required | `1` |

### Deployment configuration fields

| Field | Description | Example |
|---|---|---|
| `pivotContainerImageUrl` | OCI image URL with SHA256 digest | `ghcr.io/user/app@sha256:f813...` |
| `pivotPath` | Path to binary in the container | `/helloworld` |
| `pivotArgs` | CLI arguments as JSON array. For the helloworld template, use `["--host", "0.0.0.0", "--port", "3000"]` | `["--host", "0.0.0.0", "--port", "3000"]` |
| `expectedPivotDigest` | SHA256 hash of the **binary file** (not the container image digest) | `cbe011...` |
| `publicIngressPort` | Port exposed to the internet | `3000` |
| `healthCheckPort` | Port for health checks | `3000` |
| `healthCheckType` | Health check protocol | `TVC_HEALTH_CHECK_TYPE_HTTP` |
| `qosVersion` | QuorumOS version | `v2026.2.6` |

For troubleshooting deployment issues, see [references/deployment-troubleshooting.md](references/deployment-troubleshooting.md).

## Rules

**Code:**
- Always run tests and lint before deploying
- Never use `unwrap()`, `expect()`, or `panic!()` in production code (compiler rejects it)
- Do not modify shared middleware crates (metrics) unless adding custom metric types
- Keep the `/health` endpoint functional for orchestration and TVC health checks
- Add workspace dependencies at the workspace level first, then reference with `workspace = true`
- Use `rustls-tls` for HTTP clients (no system SSL in enclaves)
- Design for stateless, deterministic computation. Avoid relying on in-memory state across requests.
- When renaming the binary, update all references: Cargo.toml, Containerfile, Makefile, e2e test harness, and CI workflows

**Container builds:**
- Container images MUST be publicly pullable or deployed with `--pivot-pull-secret`
- Container builds must be reproducible (no non-deterministic build steps)
- Always pin container images by SHA256 digest, never by mutable tag alone
- Verify the image is public before deploying (use the ghcr.io token check in Step 3)
- The `expectedPivotDigest` is the SHA256 of the binary file, NOT the container image digest. These are different values.

**Deployment:**
- Always use `--json` flag when parsing TVC CLI output programmatically
- Set `externalConnectivity: true` in app.json for any app that needs to serve external HTTP traffic
- Always include `pivotArgs: ["--host", "0.0.0.0", "--port", "3000"]` (or your app's equivalent) so the binary listens on all interfaces
- Remove the `pivotContainerEncryptedPullSecret` field entirely from deploy.json if the image is public
- App names are permanent and cannot be deleted. Choose names carefully.
- Expect a 1-2 minute provisioning delay after approval before the app responds. A 404 during this window is normal.

**Security:**
- Never commit API keys, operator keys, or pull secrets to version control
- Never log or display private key material
- Always confirm with the user before running `tvc deploy approve --yes` (see Security section above)
- Prefer `--dry-run` to review manifests before generating approvals

## Related Skills

- `stamping-api` for constructing X-Stamp authentication headers manually
- `managing-users-api` for user lifecycle, API keys, and user tag management
- `managing-wallets-api` for wallet creation and address derivation
- `signing-transactions-api` for signing transactions
- `managing-policies-api` for access control and governance

## Related Resources

- TVC Quickstart: https://docs.turnkey.com/getting-started/verifiable-cloud-quickstart
- TVC Dashboard: https://app.turnkey.com/dashboard/tvc
- Secure Enclaves: https://docs.turnkey.com/security/secure-enclaves
- Turnkey Verified (App Proofs): https://docs.turnkey.com/security/turnkey-verified
- QuorumOS: https://docs.turnkey.com/security/quorum-deployments
- StageX (container builds): https://stagex.tools
- Axum framework: https://docs.rs/axum/latest/axum/
