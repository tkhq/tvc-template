# AGENTS.md — TVC App Template

You are an AI coding agent (Claude Code, Cursor, or similar) and your human has asked you to build or customize a Turnkey Verifiable Cloud (TVC) enclave application starting from this repo. Read this file end-to-end before touching code.

## What this repo is

A GitHub Template repo (`tkhq/tvc-template`) for building a [Turnkey Verifiable Cloud](https://docs.turnkey.com/features/verifiable-cloud/overview) enclave app. The intended workflow: create a new repo from this template on GitHub, replace the `helloworld` crate with your app, build a StageX-based OCI image, and deploy it into an AWS Nitro Enclave with the `tvc` CLI.

## When to reach for this

Use this skill when the human asks any of:

- "Build me a TVC app that does X."
- "Deploy this in a Nitro enclave with attestation."
- "Customize this template to expose an endpoint that ..."
- "Get a Turnkey Verifiable Cloud enclave running for ..."

If the request is unrelated to enclaves or TVC, stop and reconsider — this template is opinionated and not general-purpose scaffolding.

## Prerequisites (verify before building)

- **Docker >= 26 with the containerd image store enabled.** StageX builds will silently fail without it. See `README.md` for the exact Docker Desktop toggle and the `/etc/docker/daemon.json` snippet.
- **Rust toolchain** — pinned in `rust-toolchain.toml`. `cargo` will install it automatically if missing.
- **`tvc` CLI installed.** Source and install instructions: <https://github.com/tkhq/rust-sdk/tree/main/tvc>.
- **Turnkey account + org ID.** Org ID is on the Turnkey dashboard home page (click-to-copy). API credentials are set up via `tvc login`.

Ask the human for their org ID if you don't have it. Never hardcode it into source.

## Repo layout

```
crates/
  helloworld/     # REST server binary — the thing you rename & customize
  metrics/        # Prometheus metrics Tower middleware — keep as-is
  e2e/            # End-to-end test harness
images/
  helloworld/     # StageX Containerfile for the OCI image
Makefile          # build / test / run / image build targets
Cargo.toml        # workspace root — lists members + shared deps
rust-toolchain.toml
```

Inside `crates/helloworld/src/`:

- `main.rs` — binary entrypoint; loads QOS keys, wires the metrics layer, starts axum.
- `cli.rs` — clap CLI args (host, port, ephemeral/quorum key files).
- `router.rs` — axum `Router` composition; add new routes here.
- `handlers/` — one file per handler group (`basic.rs`, `btc.rs`, `dl.rs`, `keys.rs`) with a `mod.rs` re-exporting them.
- `state.rs` — `AppState` (keys, shared HTTP client, etc.).
- `response.rs` — shared response helpers.
- `client.rs` — outbound HTTP helper.

Confirm this with `ls crates/helloworld/src/` before editing — don't trust this list if the repo has evolved.

## Local dev loop

```sh
make run    # generates local QOS keys under /tmp, starts server on http://127.0.0.1:44020
make test   # cargo test --all-targets
make lint   # cargo clippy -D warnings
make fmt    # cargo fmt
```

Endpoint examples (curl commands) live in `README.md` — don't duplicate them, point the human there.

## Customization pattern

The template is designed to be forked and reshaped, not extended in place. When customizing:

1. **Rename the crate.** In `Cargo.toml` `[workspace] members`, in `crates/helloworld/Cargo.toml` `[package] name`, and in the directory name `crates/helloworld/` → `crates/<yourapp>/`. Update `images/helloworld/` → `images/<yourapp>/` and the `APPLICATION_CRATE_NAME` env in its `Containerfile`. Update the `Makefile` target `out/helloworld/index.json` and the `REGISTRY` value.
2. **Add or replace handlers.** Follow the existing shape:
   - Add a function in a file under `crates/<yourapp>/src/handlers/` (or a new module file).
   - Re-export from `handlers/mod.rs`.
   - Register the route in `router.rs` inside `router_with_state()`.
   - If it needs shared state (keys, HTTP client), read it from `AppState`; if you need new state, extend `state.rs`.
3. **Keep the metrics middleware wired the same way.** `main.rs` builds `MetricsLayer`, `.layer()`s it onto the router, then adds `/metrics` from `metrics::handler(collector)`. Do not remove this — Turnkey operators expect `/metrics`.
4. **Update the Containerfile.** Change `APPLICATION_CRATE_NAME` to your new crate name. Leave the StageX base images and static-musl build flags alone unless you know exactly why you're changing them.
5. **Update tests.** Modify `crates/e2e/tests/helloworld.rs` (or rename it) to exercise your new endpoints. The router unit tests inside `crates/<yourapp>/src/router.rs` show the `tower::ServiceExt::oneshot` pattern.

Read the existing handler files (`crates/helloworld/src/handlers/basic.rs` is the simplest) before writing your first new one — copy the signature style, don't invent your own.

## Building the enclave image

```sh
make out/helloworld/index.json
```

(Substitute `<yourapp>` for `helloworld` after renaming.) This produces an OCI image under `out/<yourapp>/` via StageX. The build runs with `--network=none` inside a sandboxed container; all dependencies must be pre-fetched by `cargo fetch`. The final stage prints the SHA-256 of the binary — you'll need it for the `tvc` deploy step.

## Deploying with `tvc`

The general sequence:

1. `tvc login` — authenticate against your Turnkey org.
2. `tvc deploy create ...` — submit the OCI image as a deployment proposal.
3. `tvc deploy status ...` — check whether the deployment needs quorum approval.
4. `tvc deploy approve ...` — approve if you're a quorum member.
5. `tvc app status ...` — verify the running app.

**Do not invent flags or subcommands.** Run `tvc <command> --help` for exact usage, and defer to <https://docs.turnkey.com/products/verifiable-cloud/onboarding> for the end-to-end onboarding walkthrough. If the human asks for exact commands and you're unsure, run `tvc --help` yourself or ask them to.

## What NOT to do

- **Do not hardcode org IDs, API keys, or credentials in source.** Read them from environment or a config file the human controls.
- **Do not skip the containerd snapshotter setup.** The image build will fail without it, often with a confusing error.
- **Do not run the enclave binary outside the container for anything security-sensitive.** Attestation, quorum-key operations, and enclave measurements only work inside the Nitro enclave. Local `make run` is for developing handler logic, not for security-relevant testing.
- **Do not add dependencies casually.** This binary runs inside an enclave with supply-chain-hardening implications. Prefer workspace deps already listed in the root `Cargo.toml`. New deps expand the enclave's attack surface and code that has to be reviewed.
- **Do not remove or reorder the `unsafe_code = "forbid"`, `unwrap_used = "deny"`, `expect_used = "deny"`, `panic = "deny"` lints in `Cargo.toml`.** They are load-bearing for enclave-code discipline. Return `Result`s; don't `unwrap`.
- **Do not modify the `/metrics` endpoint wiring** without a clear reason — Turnkey's observability tooling expects it.

## Related resources

- Concept + feature overview: <https://docs.turnkey.com/features/verifiable-cloud/overview>
- Step-by-step onboarding: <https://docs.turnkey.com/products/verifiable-cloud/onboarding>
- `tvc` CLI source: <https://github.com/tkhq/rust-sdk/tree/main/tvc>
- Larger example apps: <https://github.com/tkhq/tvc-examples>
- This repo's `README.md` for endpoint examples and Docker setup detail.
