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

$ curl localhost:44020/btc_price
{"bitcoin_usd":64225.0}

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

- `Content-Digest`
- `Signature-Input`
- `Signature`

`Content-Digest` uses RFC 9530 `sha-256=:BASE64_DIGEST:` over the exact response
body bytes. `Signature-Input` and `Signature` use the narrow RFC 9421 HTTP
Message Signatures subset this template needs: P-256 signatures only, with the
hard-coded algorithm identifier `ecdsa-p256-sha256` and fixed labels
`ephemeral` and `quorum`.

The response signature base binds the request method and path captured by the
Axum/Tower middleware, the response status, and the content digest:

```text
"@method": GET
"@path": /hello_world
"@status": 200
"content-digest": sha-256=:...:
"@signature-params": ("@method" "@path" "@status" "content-digest");created=1741048558;keyid="ephemeral";alg="ecdsa-p256-sha256"
```

Example response headers:

```http
Content-Digest: sha-256=:...:
Signature-Input: ephemeral=("@method" "@path" "@status" "content-digest");created=1741048558;keyid="ephemeral";alg="ecdsa-p256-sha256", quorum=("@method" "@path" "@status" "content-digest");created=1741048558;keyid="quorum";alg="ecdsa-p256-sha256"
Signature: ephemeral=:...:, quorum=:...:
```

Verifiers should use public keys they already trust from setup or attestation,
not a public key supplied by the signed response. Minimal verification looks
like:

```rust
let digest = format!("sha-256=:{}:", base64::prelude::BASE64_STANDARD.encode(
    sha2::Sha256::digest(response_body_bytes),
));
assert_eq!(content_digest_header, digest);

let signature_base = format!(
    "\"@method\": {method}\n\"@path\": {path}\n\"@status\": {status}\n\"content-digest\": {digest}\n\"@signature-params\": {ephemeral_signature_input}"
);
let signature = base64::prelude::BASE64_STANDARD.decode(ephemeral_signature_value)?;
trusted_ephemeral_public_key.verify(signature_base.as_bytes(), &signature)?;
```

`qos_json` is still useful for canonical JSON response bodies, but it is not the
HTTP response signing payload.

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
