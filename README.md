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
{"time":"1741048558"}

$ curl localhost:44020/random_app_proof
{"payload":{"random_number":"12345"},"proof":{"public_key":"...","payload":"{\"random_number\":\"12345\"}","signature":"..."}}

$ curl -X POST \
  -H 'content-type: application/json' \
  -d '{"plaintext":"hello TVC world"}' \
  localhost:44020/quorum_key/encrypt
{"ciphertext":"..."}

$ curl -X POST \
  -H 'content-type: application/json' \
  -d '{"ciphertext":"..."}' \
  localhost:44020/quorum_key/decrypt
{"plaintext":"hello TVC world"}

$ curl -X POST -d 'hello' localhost:44020/echo
hello

$ curl localhost:44020/metrics
# HELP tvc_http_request_duration_ms HTTP request duration in milliseconds
# TYPE tvc_http_request_duration_ms histogram
tvc_http_request_duration_ms_bucket{method="GET",path="/health",status="200",le="1"} 1
...
```

JSON endpoint responses are serialized through `tvc_axum::QosJson`, which uses
`qos_json` canonical JSON bytes rather than `serde_json` response serialization.
Integer values are emitted in the `qos_json` canonical decimal-string form.

Every response, including raw `/echo` bodies and Prometheus `/metrics` text,
includes these headers:

- `x-tvc-ephemeral-public-key`
- `x-tvc-response-signature`

Both values are hex-encoded. The signature verifies over the exact response body
bytes using the `x-tvc-ephemeral-public-key` public key and `qos_p256`.
Minimal verification looks like:

```rust
let public_key = P256Public::from_bytes(&qos_hex::decode(public_key_header)?)?;
let signature = qos_hex::decode(signature_header)?;
public_key.verify(&response_body_bytes, &signature)?;
```

See `crates/e2e/tests/helloworld.rs` for an end-to-end verification example.

## Development

### Run tests

```
make test
```

### Run locally

```
make run
```

Server starts on http://127.0.0.1:44020

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

## Project Structure

```
crates/
  helloworld/     # REST server binary
  metrics/        # Prometheus metrics Tower middleware
  tvc-axum/       # Axum QosJson response and response-signing adapters
  e2e/            # End-to-end tests
images/
  helloworld/     # Containerfile for OCI image
```
