# TVC CLI Reference

Complete reference for the `tvc` command-line tool. The CLI supports fully autonomous, non-interactive operation for agent-driven workflows and CI/CD pipelines.

## Installation

```bash
cd rust-sdk/tvc && cargo install --path .
```

## Global Flags

These flags are available on ALL commands:

| Flag | Env Var | Description |
|---|---|---|
| `--json` | `TVC_JSON` | Output results as JSON to stdout (use for programmatic parsing) |
| `--no-input` | `TVC_NO_INPUT` | Disable all interactive prompts. Fails if input is required. |
| `--quiet` / `-q` | | Suppress non-essential output |
| `--api-key-file <PATH>` | `TVC_API_KEY_FILE` | Path to API key JSON file (overrides login config) |
| `--operator-key-file <PATH>` | `TVC_OPERATOR_KEY_FILE` | Path to operator key JSON file (overrides login config) |
| `--api-url <URL>` | `TVC_API_URL` | API base URL override |
| `--org-id <ID>` | `TVC_ORG_ID` | Organization ID override |

When `--api-key-file`, `--api-url`, and `--org-id` are ALL provided, commands work without running `tvc login` first.

## Commands

### tvc login

Authenticate with Turnkey and set up local credentials.

```bash
# Interactive (first-time setup)
tvc login

# Select existing org
tvc login --org my-alias

# Fully non-interactive (CI/CD)
tvc login --no-input --org-id <ORG_UUID> --alias default --api-env prod --skip-api-key-wait
```

| Flag | Env Var | Description |
|---|---|---|
| `--org <ALIAS_OR_ID>` | | Select an existing org by alias or ID |
| `--alias <NAME>` | `TVC_ORG_ALIAS` | Alias for the org config (default: "default") |
| `--api-env <ENV>` | `TVC_API_ENV` | API environment: `prod`, `preprod`, `dev`, `local` |
| `--skip-api-key-wait` | | Skip the "press Enter" prompt after API key generation |

### tvc app init

Generate a template app configuration file.

```bash
tvc app init --output my-app.json
```

| Flag | Description |
|---|---|
| `-o, --output <PATH>` | Output file path (default: `app.json`) |

The template contains `<FILL_IN_...>` placeholders for `name` and `manifestSetParams.name` that MUST be replaced before creating the app. The operator public key and quorum public key are auto-populated from login.

### tvc app create

Create a new TVC application from a config file.

```bash
tvc app create my-app.json

# With JSON output for parsing
APP_RESULT=$(tvc --json app create my-app.json)
APP_ID=$(echo "$APP_RESULT" | jq -r '.app_id')
OPERATOR_ID=$(echo "$APP_RESULT" | jq -r '.manifest_set_operator_ids[0]')
```

Returns: app_id, manifest_set_id, manifest_set_operator_ids. The app ID and operator IDs are cached in `~/.config/turnkey/tvc.config.toml` for convenience.

### tvc app list

List applications in the current org.

```bash
tvc app list

# Filter by name
tvc app list --name my-app

# JSON output
tvc --json app list
```

| Flag | Description |
|---|---|
| `-n, --name <NAME>` | Filter by app name |

### tvc deploy init

Generate a template deployment configuration file.

```bash
tvc deploy init --output my-deploy.json
```

| Flag | Description |
|---|---|
| `-o, --output <PATH>` | Output file path |

The `appId` field is auto-filled from the last created app.

### tvc deploy create

Create a new deployment from a config file.

```bash
tvc deploy create my-deploy.json

# With pull secret for private container images
tvc deploy create my-deploy.json --pivot-pull-secret ./pull-secret.json

# With JSON output
DEPLOY_RESULT=$(tvc --json deploy create my-deploy.json)
DEPLOY_ID=$(echo "$DEPLOY_RESULT" | jq -r '.deployment_id')
```

| Flag | Description |
|---|---|
| `<CONFIG_FILE>` | Path to the deployment configuration file (JSON) |
| `--pivot-pull-secret <PATH>` | Path to unencrypted pull secret file (auto-encrypted by CLI) |

### tvc deploy approve

Cryptographically approve a deployment manifest.

```bash
# Interactive (reviews each manifest section)
tvc deploy approve --deploy-id <DEPLOYMENT_ID>

# Fully autonomous (skips all prompts)
tvc --json --no-input deploy approve \
  --deploy-id <DEPLOYMENT_ID> \
  --operator-id <OPERATOR_ID> \
  --yes

# Dry run (review without generating approval)
tvc deploy approve --deploy-id <DEPLOYMENT_ID> --dry-run

# Generate approval without posting to API
tvc deploy approve --deploy-id <DEPLOYMENT_ID> --yes --skip-post
```

| Flag | Env Var | Description |
|---|---|---|
| `-d, --deploy-id <ID>` | `TVC_DEPLOY_ID` | Deployment ID (fetches manifest from API) |
| `-m, --manifest <PATH>` | | Path to local manifest file |
| `--manifest-id <ID>` | `TVC_MANIFEST_ID` | Manifest UUID (required with `--manifest`) |
| `--operator-id <ID>` | `TVC_OPERATOR_ID` | Operator UUID (auto-used from config if omitted) |
| `--operator-seed <PATH>` | | Custom operator key file |
| `-y, --yes` | | Skip all interactive approval prompts |
| `--dry-run` | | Review manifest without generating approval |
| `--skip-post` | | Don't post approval to API |
| `-o, --output <PATH>` | | Write approval to file |

### tvc deploy status

Get the status of a deployment.

```bash
tvc deploy status --deploy-id <DEPLOYMENT_ID>

# JSON output
STAGE=$(tvc --json deploy status --deploy-id <DEPLOY_ID> | jq -r '.stage')
```

| Flag | Env Var | Description |
|---|---|---|
| `-d, --deploy-id <ID>` | `TVC_DEPLOY_ID` | Deployment ID |

## Full Autonomous Deployment Pipeline

```bash
#!/bin/bash
set -euo pipefail

# Option A: Use existing login
# (run `tvc login` once interactively, then all subsequent commands work)

# Option B: Override flags (no login needed)
export TVC_API_KEY_FILE="/path/to/api_key.json"
export TVC_API_URL="https://api.turnkey.com"
export TVC_ORG_ID="your-org-uuid"
export TVC_JSON=true

# 1. Create the app
APP_RESULT=$(tvc app create app-config.json)
APP_ID=$(echo "$APP_RESULT" | jq -r '.app_id')
OPERATOR_ID=$(echo "$APP_RESULT" | jq -r '.manifest_set_operator_ids[0]')

# 2. Create the deployment
DEPLOY_RESULT=$(tvc deploy create deploy-config.json)
DEPLOY_ID=$(echo "$DEPLOY_RESULT" | jq -r '.deployment_id')

# 3. Approve non-interactively
tvc --no-input deploy approve \
  --deploy-id "$DEPLOY_ID" \
  --operator-id "$OPERATOR_ID" \
  --yes

# 4. Verify status (enclave may take 1-2 minutes to provision)
tvc deploy status --deploy-id "$DEPLOY_ID"

# 5. App URL depends on environment (check api_base_url in tvc.config.toml)
# Production: https://app-${APP_ID}.turnkey.cloud
# Dev:        https://app-${APP_ID}.tvc.dev.turnkey.engineering
```

## Configuration

Config files are stored at `~/.config/turnkey/`:

```
~/.config/turnkey/
  tvc.config.toml          # Main config (orgs, active org, cached IDs)
  orgs/<alias>/
    api_key.json            # API key (public + private)
    operator.json           # Operator key (public + private)
```

### Cached state in tvc.config.toml

The CLI caches useful IDs for convenience:
- `last_created_app_id`: Auto-filled in `deploy init` templates
- `last_operator_ids`: Auto-used in `deploy approve` when `--operator-id` is omitted

## Environment Variables

| Variable | Used By | Description |
|---|---|---|
| `TVC_JSON` | Global | Enable JSON output |
| `TVC_NO_INPUT` | Global | Disable interactive prompts |
| `TVC_API_KEY_FILE` | Global | Path to API key JSON file |
| `TVC_OPERATOR_KEY_FILE` | Global | Path to operator key JSON file |
| `TVC_API_URL` | Global | API base URL |
| `TVC_ORG_ID` | Global, Login | Organization ID |
| `TVC_ORG_ALIAS` | Login | Organization alias |
| `TVC_API_ENV` | Login | API environment name |
| `TVC_DEPLOY_ID` | Deploy approve/status | Deployment ID |
| `TVC_MANIFEST_ID` | Deploy approve | Manifest ID |
| `TVC_OPERATOR_ID` | Deploy approve | Operator ID |

## API Environments

| Environment | API URL | App URL Pattern |
|---|---|---|
| Production | `https://api.turnkey.com` | `https://app-<UUID>.turnkey.cloud` |
| Preprod | `https://api.preprod.turnkey.engineering` | Check with team |
| Dev | `https://api.dev.turnkey.engineering` | `https://app-<UUID>.tvc.dev.turnkey.engineering` |
| Local | `http://localhost:8081` | N/A |
