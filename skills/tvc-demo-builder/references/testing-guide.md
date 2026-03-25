# Testing Guide

Comprehensive testing patterns for TVC demo applications.

## Test Hierarchy

### 1. Unit Tests (in router.rs)

Fast, no server needed. Use Tower's `oneshot()` to send requests directly to the router.

**When to use:** Testing individual handler logic, input validation, error paths.

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    // Helper: extract body as string
    async fn body_string(body: Body) -> String {
        let bytes = body.collect().await.expect("failed to read body").to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
    }

    // Helper: extract body as JSON
    async fn body_json(body: Body) -> serde_json::Value {
        let s = body_string(body).await;
        serde_json::from_str(&s).expect("response is not valid JSON")
    }

    // Test a GET endpoint
    #[tokio::test]
    async fn test_get_endpoint() {
        let app = router();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/your_endpoint")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["key"], "expected_value");
    }

    // Test a POST endpoint with JSON body
    #[tokio::test]
    async fn test_post_endpoint() {
        let app = router();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/your_endpoint")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"field":"value"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
    }

    // Test error handling (bad input)
    #[tokio::test]
    async fn test_bad_input_returns_400() {
        let app = router();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/your_endpoint")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{}"#))  // Missing required fields
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 400);
        let json = body_json(response.into_body()).await;
        assert!(json["error"].is_string(), "should return error message");
    }

    // Test path parameters
    #[tokio::test]
    async fn test_path_parameter() {
        let app = router();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/price/ETH")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let json = body_json(response.into_body()).await;
        assert_eq!(json["asset"], "ETH");
    }
}
```

### 2. E2E Tests (in src/e2e/tests/)

Spawns the real server binary, makes real HTTP requests. Tests the full stack including middleware.

**When to use:** Testing the complete request/response cycle, middleware behavior, metrics recording.

```rust
// src/e2e/tests/your_demo.rs
#![allow(missing_docs, clippy::unwrap_used)]
use e2e::TestArgs;

#[tokio::test]
async fn test_demo_endpoint_e2e() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();

        // Test your endpoint
        let resp = client
            .post(format!("{}/your_endpoint", test_args.base_url))
            .json(&serde_json::json!({"field": "value"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let json: serde_json::Value = resp.json().await.unwrap();
        assert!(json["result"].is_string());
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_metrics_include_demo_endpoint() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();

        // Hit the demo endpoint first
        client
            .get(format!("{}/your_endpoint", test_args.base_url))
            .send()
            .await
            .unwrap();

        // Verify it shows up in metrics
        let resp = client
            .get(format!("{}/metrics", test_args.base_url))
            .send()
            .await
            .unwrap();
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("path=\"/your_endpoint\""),
            "demo endpoint should appear in metrics"
        );
    }
    e2e::Builder::new().execute(test).await;
}
```

### 3. Manual Testing with curl

After `make -C src run`:

```bash
# Health check
curl -s localhost:44020/health | jq

# Your demo endpoint (GET)
curl -s localhost:44020/price/ETH | jq

# Your demo endpoint (POST)
curl -s -X POST localhost:44020/sign \
  -H "Content-Type: application/json" \
  -d '{"amount": 100, "destination": "0xabc..."}' | jq

# Metrics (verify your endpoint shows up)
curl -s localhost:44020/metrics | grep your_endpoint
```

## Test Patterns for Common Scenarios

### Testing External API Calls

For demos that fetch external data (price oracles), either:

1. **Mock at the HTTP level** in unit tests (use `axum::Router` as a mock server)
2. **Test with real APIs** in e2e tests (accept that they may be flaky)
3. **Extract pure logic** into functions that take data as input, test those directly

```rust
// Extract pure business logic for testable units
fn compute_median(prices: &[f64]) -> f64 {
    let mut sorted = prices.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

#[test]
fn test_median_odd() {
    assert_eq!(compute_median(&[1.0, 3.0, 2.0]), 2.0);
}

#[test]
fn test_median_even() {
    assert_eq!(compute_median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
}
```

### Testing Stateful Endpoints

For demos with in-memory state (auctions, policy configs):

```rust
#[tokio::test]
async fn test_stateful_workflow() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let base = &test_args.base_url;

        // Step 1: Configure
        let resp = client
            .post(format!("{base}/policy/configure"))
            .json(&serde_json::json!({"max_amount": 1000}))
            .send().await.unwrap();
        assert_eq!(resp.status(), 200);

        // Step 2: Test within limits (should pass)
        let resp = client
            .post(format!("{base}/sign"))
            .json(&serde_json::json!({"amount": 500}))
            .send().await.unwrap();
        assert_eq!(resp.status(), 200);

        // Step 3: Test over limits (should fail)
        let resp = client
            .post(format!("{base}/sign"))
            .json(&serde_json::json!({"amount": 5000}))
            .send().await.unwrap();
        assert_eq!(resp.status(), 403);
    }
    e2e::Builder::new().execute(test).await;
}
```

## Commands Reference

```bash
# Run all tests (unit + e2e)
make -C src test

# Run only unit tests (faster)
cd src && cargo test --lib

# Run only e2e tests
cd src && cargo test --test '*'

# Run a specific test by name
cd src && cargo test test_your_endpoint

# Run tests with output (see println! in tests)
cd src && cargo test -- --nocapture

# Lint (catches safety violations, missing docs)
make -C src lint

# Format code
make -C src fmt

# Run locally for manual curl testing
make -C src run
```
