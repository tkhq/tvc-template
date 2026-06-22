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

$ curl localhost:44020/attestation
{"manifestEnvelope":{"manifest":{"namespace":{...},"pivot":{...},"manifestSet":{...},"shareSet":{...},"enclave":{...},"patchSet":{...}},"manifestSetApprovals":[],"shareSetApprovals":[],"manifestHash":"..."},"attestationDoc":"..."}

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

For local development outside Nitro, pass `--unsafe-mock-nsm` to run with
`qos_nsm::mock::DynamicMockNsm::new().with_mock_certificate_chain()`. This flag
is not production-safe; production uses the real `qos_nsm::Nsm` provider by
default.

### Attestation endpoint

`GET /attestation` returns the QOS manifest envelope currently available to the
app plus a live Nitro-style COSE Sign1 attestation document:

```json
{
  "manifestEnvelope": {
    "manifest": { "...": "QOS manifest fields" },
    "manifestSetApprovals": [],
    "shareSetApprovals": [],
    "manifestHash": "hex-encoded manifest hash"
  },
  "attestationDoc": "hex-encoded COSE_Sign1 bytes"
}
```

The endpoint does not generate or manage the ephemeral key. It loads the normal
app/QOS ephemeral public key and passes it to `NsmRequest::Attestation`. The
attestation document's `user_data` is the manifest hash, and its `public_key` is
the app ephemeral public key.

Router tests construct `AppState` directly with an injected
`qos_nsm::mock::DynamicMockNsm`; they do not exercise CLI parsing. Full
verification tests use `with_mock_certificate_chain()` and
`mock_root_certificate_der()` to verify the returned COSE Sign1 document against
the mock root certificate, and assert that verification against the AWS root
fails.

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
