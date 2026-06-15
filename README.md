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

$ curl localhost:44020/random_app_proof
{"random_number":"12345","proof":{"public_key":"...","payload":"{\"random_number\":\"12345\"}","signature":"..."}}

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

## Response signatures

Every response is signed with the enclave's ephemeral
[`qos_p256`](https://crates.io/crates/qos_p256) key (the same key used by
`/random_app_proof`). A Tower middleware layer buffers each response body,
signs the exact body bytes, and attaches two hex-encoded headers without
otherwise changing the status, body, or content type:

| Header | Contents |
| --- | --- |
| `x-tvc-ephemeral-public-key` | Hex of the ephemeral public key (`P256Public::to_bytes()`, uncompressed SEC1). |
| `x-tvc-response-signature` | Hex of the qos_p256 signature over the raw response body bytes. |

This applies to all endpoints, including `/metrics`.

```sh
$ curl -i localhost:44020/hello_world
HTTP/1.1 200 OK
content-type: application/json
x-tvc-ephemeral-public-key: 04a1b2c3...
x-tvc-response-signature: 3045022100...
...
{"message":"hello world"}
```

To verify a response, hex-decode both headers and check the signature over the
exact response body bytes:

```rust
use qos_p256::P256Public;

let public_key = P256Public::from_bytes(&qos_hex::decode(public_key_header)?)?;
let signature = qos_hex::decode(signature_header)?;
public_key.verify(response_body_bytes, &signature)?;
```

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
  e2e/            # End-to-end tests
images/
  helloworld/     # Containerfile for OCI image
```
