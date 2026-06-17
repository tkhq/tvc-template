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

- `x-tvc-ephemeral-signature`
- `x-tvc-quorum-signature`
- `x-tvc-signature-timestamp`

Signature values are hex-encoded. The timestamp is a Unix UTC timestamp included
because the server opts into timestamped response signing. Signatures verify over
a `qos_json` canonical signing payload, not directly over the response body:

```json
{"body":"<hex response body>","timestamp":"<x-tvc-signature-timestamp>"}
```

Verifiers should use public keys they already trust from setup or attestation,
not a public key supplied by the signed response. Minimal verification looks
like:

```rust
#[derive(serde::Serialize)]
struct TimestampedPayload {
    #[serde(with = "qos_hex::serde")]
    body: Vec<u8>,
    #[serde(with = "qos_json::string_or_numeric")]
    timestamp: u64,
}

let payload = qos_json::to_vec(&TimestampedPayload {
    body: response_body_bytes.to_vec(),
    timestamp: signature_timestamp_header.parse()?,
})?;
let signature = qos_hex::decode(ephemeral_signature_header)?;
trusted_ephemeral_public_key.verify(&payload, &signature)?;
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
