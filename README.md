# TVC App Template

A starter template for building [Turnkey Verifiable Cloud (TVC)](https://docs.turnkey.com) enclave applications.

This is a minimal REST server that demonstrates the structure and patterns for running an application inside a TVC enclave.

## Endpoints

```sh
$ curl localhost:44020/health
{"status":"healthy"}

$ curl localhost:44020/hello_world
{"message":"hello world"}

$ curl localhost:44020/time
{"time":1741048558}

$ curl -X POST -d 'hello' localhost:44020/echo
hello

$ curl localhost:44020/metrics
# HELP tvc_http_request_duration_ms HTTP request duration in milliseconds
# TYPE tvc_http_request_duration_ms histogram
tvc_http_request_duration_ms_bucket{method="GET",path="/health",status="200",le="1"} 1
...
```

## Development

### Run tests

```
make -C src test
```

### Run locally

```
make -C src run
```

Server starts on http://127.0.0.1:44020

### Lint

```
make -C src lint
```

## Building OCI containers

This repository uses [StageX](https://stagex.tools) to build OCI containers. Requires Docker >= 26 with containerd:

- **Docker Desktop:** Dashboard > Settings > "Use containerd for pulling and storing images"
- **Linux:** add to `/etc/docker/daemon.json`:
  ```json
  {
    "features": {
      "containerd-snapshotter": true
    }
  }
  ```

Build the container:

```sh
make out/helloworld/index.json
```

## Deploying to TVC

See the [TVC Quickstart](https://docs.turnkey.com/getting-started/verifiable-cloud-quickstart) for full instructions.

**Quick reference (CLI):**
```sh
# Install the TVC CLI
git clone git@github.com:tkhq/rust-sdk.git && cd rust-sdk && cargo install --path tvc

# Login (import existing API key from dashboard)
tvc login --org-id <ORG_UUID> --api-key-file /path/to/credentials.json --api-env prod

# Create app (set externalConnectivity to true for HTTP apps)
tvc app init --output app.json
tvc app create app.json

# Build, push container, then deploy
make out/helloworld/index.json
tvc deploy init --output deploy.json
tvc deploy create deploy.json
tvc deploy approve --deploy-id <DEPLOY_ID>
```

**Key settings:**
- Set `"externalConnectivity": true` in app.json for any app serving external traffic
- Set `pivotArgs` to `["--host", "0.0.0.0", "--port", "3000"]` so the binary listens on all interfaces
- The `expectedPivotDigest` in deploy.json is the SHA256 of the binary file, not the container image digest

## Security Considerations

### Credential handling

API keys and operator keys are P256 keypairs with signing authority over your Turnkey organization. Handle them with the same care as production database credentials.

- Never commit API key files (`api_key.json`), operator key files (`operator.json`), or pull secrets to version control
- The CLI stores credentials at `~/.config/turnkey/orgs/<alias>/`. Do not copy or share these files.
- Rotate API keys periodically. Generate a new key, verify it works, then delete the old one.

### Agent-assisted deployment

If you use an AI agent (e.g., Claude Code with the `tvc-app-builder` skill) to deploy TVC applications, be aware of the following:

- **Manifest approval is a cryptographic action.** The `tvc deploy approve --yes` command signs and submits a deployment manifest. This is equivalent to signing a transaction. Always review the manifest before approving.
- **Use `--dry-run` first.** Run `tvc deploy approve --dry-run` to review the manifest contents before generating an actual approval.
- **Apps cannot be deleted.** There is currently no way to delete a TVC app through the CLI or dashboard. Choose app names carefully.
- **Verify before trusting "Live" status.** A deployment showing "Live" on the dashboard means the API accepted it, but the app may not be reachable yet (1-2 minute provisioning delay is normal). Always verify by hitting the health endpoint.

### Container image security

- Always pin container images by SHA256 digest in `pivotContainerImageUrl`, never by mutable tag alone
- Verify your image is publicly pullable before deploying, or provide a pull secret
- Container builds are reproducible via StageX. The same source code produces the same binary hash, which is verified by the enclave at boot.

## Project Structure

```
src/
  helloworld/     # REST server binary
  metrics/        # Prometheus metrics Tower middleware
  e2e/            # End-to-end tests
images/
  helloworld/     # Containerfile for OCI image
skills/
  tvc-app-builder/ # Claude Code skill for building and deploying TVC apps
```
