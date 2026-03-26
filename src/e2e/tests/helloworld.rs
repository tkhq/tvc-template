#![allow(missing_docs, clippy::unwrap_used)]

use e2e::TestArgs;

#[tokio::test]
async fn test_health() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/health", test_args.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["status"], "healthy");
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_hello_world() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/hello_world", test_args.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["message"], "hello world");
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_time() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/time", test_args.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert!(
            json["time"].is_u64(),
            "time field should be a unix timestamp"
        );
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_echo() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/echo", test_args.base_url))
            .body("hello echo")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert_eq!(body, "hello echo");
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_echo_json() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let sent = serde_json::json!({"foo": "bar", "count": 42});
        let resp = client
            .post(format!("{}/echo", test_args.base_url))
            .json(&sent)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let received: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(received, sent);
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_metrics() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();

        // Hit an endpoint first so the histogram has data
        client
            .get(format!("{}/health", test_args.base_url))
            .send()
            .await
            .unwrap();

        let resp = client
            .get(format!("{}/metrics", test_args.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let content_type = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            content_type.starts_with("text/plain"),
            "expected prometheus text format content type, got: {content_type}"
        );

        let body = resp.text().await.unwrap();
        assert!(
            body.contains("tvc_http_request_duration_ms"),
            "should contain the namespaced histogram metric"
        );
        assert!(
            body.contains("method=\"GET\""),
            "should contain method label"
        );
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_notarize_and_verify() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let base = &test_args.base_url;

        // Notarize a document hash
        let resp = client
            .post(format!("{base}/notarize"))
            .json(&serde_json::json!({"hash": "sha256:deadbeef"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["hash"], "sha256:deadbeef");
        assert!(json["timestamp"].is_u64());
        let receipt_id = json["receipt_id"].as_u64().unwrap();

        // Verify the receipt
        let resp = client
            .get(format!("{base}/verify/{receipt_id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["hash"], "sha256:deadbeef");
        assert_eq!(json["receipt_id"], receipt_id);
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_verify_nonexistent_returns_404() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/verify/99999", test_args.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_stats_reflects_notarizations() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let base = &test_args.base_url;

        // Stats should start at 0
        let resp = client
            .get(format!("{base}/stats"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["total_notarized"], 0);

        // Notarize three documents
        for hash in &["hash_a", "hash_b", "hash_c"] {
            let resp = client
                .post(format!("{base}/notarize"))
                .json(&serde_json::json!({"hash": hash}))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 200);
        }

        // Stats should now be 3
        let resp = client
            .get(format!("{base}/stats"))
            .send()
            .await
            .unwrap();
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["total_notarized"], 3);
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_notarize_empty_hash_returns_400() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/notarize", test_args.base_url))
            .json(&serde_json::json!({"hash": ""}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }
    e2e::Builder::new().execute(test).await;
}
