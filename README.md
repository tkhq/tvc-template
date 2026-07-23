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

$ curl -X POST \
  -H 'content-type: application/json' \
  -d '{"organizationId":"org-123","signWith":"0xabc0000000000000000000000000000000000001","unsignedTransaction":"0xdeadbeef","timestampMs":1700000000000}' \
  localhost:44020/sign_turnkey_transaction
{"activityBody":"{\"type\":\"ACTIVITY_TYPE_SIGN_TRANSACTION_V2\",\"timestampMs\":\"1700000000000\",\"organizationId\":\"org-123\",\"parameters\":{\"signWith\":\"0xabc0000000000000000000000000000000000001\",\"unsignedTransaction\":\"deadbeef\",\"type\":\"TRANSACTION_TYPE_ETHEREUM\"}}","xStamp":"..."}

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

### Forward a signed Turnkey request

`POST /sign_turnkey_transaction` derives a domain-separated P-256 API key from
the TVC quorum key and signs the exact `activityBody` returned in the response.
Register the `publicKey` inside the decoded `xStamp` envelope as an
`API_KEY_CURVE_P256` credential for a Turnkey API user before using it. The
derived key is stable for a given quorum key.

The enclave does not submit the request to Turnkey. Forward both returned values
without reserializing `activityBody`:

```sh
response=$(curl -sS -X POST \
  -H 'content-type: application/json' \
  -d '{"organizationId":"org-123","signWith":"0xabc0000000000000000000000000000000000001","unsignedTransaction":"0xdeadbeef","timestampMs":1700000000000}' \
  localhost:44020/sign_turnkey_transaction)

activity_body=$(printf '%s' "$response" | jq -r '.activityBody')
x_stamp=$(printf '%s' "$response" | jq -r '.xStamp')

curl -X POST \
  -H 'content-type: application/json' \
  -H "X-Stamp: $x_stamp" \
  --data-binary "$activity_body" \
  https://api.turnkey.com/public/v1/submit/sign_transaction
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
