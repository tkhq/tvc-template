# TVC App Template

A starter template for building [Turnkey Verifiable Cloud (TVC)](https://docs.turnkey.com) enclave applications.

This is a minimal REST server that demonstrates the structure and patterns for running an application inside a TVC enclave.

## Endpoints

```sh
$ curl localhost:3000/health
{"status":"healthy"}

$ curl localhost:3000/hello_world
{"message":"hello world"}

$ curl localhost:3000/time
{"time":1741048558}

$ curl localhost:3000/random_app_proof
{"random_number":"12345","proof":{"public_key":"...","payload":"{\"random_number\":\"12345\"}","signature":"..."}}

$ curl -X POST \
  -H 'content-type: application/json' \
  -d '{"plaintext":"hello TVC world"}' \
  localhost:3000/quorum_key/encrypt
{"ciphertext":"..."}

$ curl -X POST \
  -H 'content-type: application/json' \
  -d '{"ciphertext":"..."}' \
  localhost:3000/quorum_key/decrypt
{"plaintext":"hello TVC world"}

$ curl -X POST -d 'hello' localhost:3000/echo
hello

$ curl localhost:3000/btc_price
{"bitcoin_usd":64225.0}

# Fetch an arbitrary URL through the enclave's verified-TLS egress. Handy for
# testing egress against different hosts. A scheme-less host defaults to https.
$ curl 'localhost:3000/verified_tls_get?url=example.com'
{"requested_url":"https://example.com","final_url":"https://example.com/","status":200,"headers":{...},"body":"...","body_truncated":false}

$ curl localhost:3000/metrics
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

Server starts on http://127.0.0.1:3000

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
