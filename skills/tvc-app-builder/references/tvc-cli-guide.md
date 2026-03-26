# TVC CLI Reference

Complete reference for the `tvc` command-line tool. The CLI supports non-interactive operation for agent-driven workflows via `--dangerous-skip-interactive` on the approve command and cached config state.

## Installation

```bash
cd rust-sdk/tvc && cargo install --path .
```

## Output Format

The CLI outputs human-readable text, not JSON. To extract IDs programmatically, parse labeled output lines:

```bash
# Example: capture App ID from `tvc app create` output
OUTPUT=$(tvc app create app.json 2>&1)
APP_ID=$(echo "$OUTPUT" | grep "App ID:" | awk '{print $NF}')
OPERATOR_ID=$(echo "$OUTPUT" | grep "Manifest Set Operator IDs:" | awk '{print $NF}')
```

Alternatively, read cached IDs from `~/.config/turnkey/tvc.config.toml` after commands complete. The CLI caches `last_created_app_id` and `last_operator_ids` automatically.

## Commands

### tvc login

Authenticate with Turnkey and set up local credentials. This is an interactive process that walks through org creation/selection and key generation.

```bash
# Interactive (first-time setup)
tvc login

# Select an existing org by alias or ID
tvc login --org my-alias
```

| Flag | Description |
|---|---|
| `--org <ALIAS_OR_ID>` | Select an existing org by alias or ID |

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
```

| Flag | Description |
|---|---|
| `<CONFIG_FILE>` | Path to the app configuration file (JSON) |

**Output includes:** App ID, Name, Manifest Set ID, Manifest Set Operator IDs. These are also cached in `~/.config/turnkey/tvc.config.toml`.

### tvc app list

List applications in the current org.

```bash
tvc app list

# Filter by name
tvc app list --name my-app
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
| `-o, --output <PATH>` | Output file path (default: `deploy.json`) |

The `appId` field is auto-filled from the last created app.

### tvc deploy create

Create a new deployment from a config file.

```bash
tvc deploy create my-deploy.json

# With pull secret for private container images
tvc deploy create my-deploy.json --pivot-pull-secret ./pull-secret.json
```

| Flag | Description |
|---|---|
| `<CONFIG_FILE>` | Path to the deployment configuration file (JSON) |
| `--pivot-pull-secret <PATH>` | Path to unencrypted pull secret file (auto-encrypted by CLI) |

**Output includes:** Deployment ID, App ID.

### tvc deploy approve

Cryptographically approve a deployment manifest.

```bash
# Interactive (reviews each manifest section with prompts)
tvc deploy approve --deploy-id <DEPLOYMENT_ID>

# Non-interactive (skips all manifest review prompts)
tvc deploy approve \
  --deploy-id <DEPLOYMENT_ID> \
  --operator-id <OPERATOR_ID> \
  --dangerous-skip-interactive

# Dry run (review without generating approval)
tvc deploy approve --deploy-id <DEPLOYMENT_ID> --dry-run

# Generate approval without posting to API
tvc deploy approve --deploy-id <DEPLOYMENT_ID> --dangerous-skip-interactive --skip-post
```

| Flag | Env Var | Description |
|---|---|---|
| `-d, --deploy-id <ID>` | `TVC_DEPLOY_ID` | Deployment ID (fetches manifest from API) |
| `-m, --manifest <PATH>` | | Path to local manifest file |
| `--manifest-id <ID>` | `TVC_MANIFEST_ID` | Manifest UUID (required with `--manifest`) |
| `--operator-id <ID>` | `TVC_OPERATOR_ID` | Operator UUID (auto-used from config if omitted) |
| `--operator-seed <PATH>` | | Custom operator key file |
| `--dangerous-skip-interactive` | | Skip all interactive manifest review prompts |
| `--dry-run` | | Review manifest without generating approval |
| `--skip-post` | | Don't post approval to API |
| `-o, --output <PATH>` | | Write approval to file |

### tvc deploy status

Get the status of a deployment.

```bash
tvc deploy status --deploy-id <DEPLOYMENT_ID>
```

| Flag | Env Var | Description |
|---|---|---|
| `-d, --deploy-id <ID>` | `TVC_DEPLOY_ID` | Deployment ID |

**Output includes:** Deployment ID, App ID, Manifest ID, QOS Version, Stage, Pivot Container details.

## Full Autonomous Deployment Pipeline

```bash
#!/bin/bash
set -euo pipefail

# Prerequisite: run `tvc login` once interactively to set up credentials

# 1. Create the app
OUTPUT=$(tvc app create app-config.json 2>&1)
echo "$OUTPUT"
APP_ID=$(echo "$OUTPUT" | grep "App ID:" | awk '{print $NF}')
OPERATOR_ID=$(echo "$OUTPUT" | grep "Manifest Set Operator IDs:" | awk '{print $NF}')

# 2. Create the deployment
OUTPUT=$(tvc deploy create deploy-config.json 2>&1)
echo "$OUTPUT"
DEPLOY_ID=$(echo "$OUTPUT" | grep "Deployment ID:" | awk '{print $NF}')

# 3. Approve non-interactively
tvc deploy approve \
  --deploy-id "$DEPLOY_ID" \
  --operator-id "$OPERATOR_ID" \
  --dangerous-skip-interactive

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

### Environment Variables

Some subcommand flags accept environment variable overrides:

| Variable | Used By | Description |
|---|---|---|
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
