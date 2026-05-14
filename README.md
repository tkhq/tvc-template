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

### Lint

```
make lint
```

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
src/
  helloworld/     # REST server binary
  metrics/        # Prometheus metrics Tower middleware
  e2e/            # End-to-end tests
images/
  helloworld/     # Containerfile for OCI image
```
