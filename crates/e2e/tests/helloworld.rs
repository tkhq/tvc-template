#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use e2e::TestArgs;
use qos_p256::P256Public;

async fn verified_body(resp: reqwest::Response) -> Vec<u8> {
    let public_key = resp
        .headers()
        .get("x-tvc-ephemeral-public-key")
        .expect("public key header should exist")
        .to_str()
        .expect("public key header should be ascii")
        .to_owned();
    let signature = resp
        .headers()
        .get("x-tvc-response-signature")
        .expect("signature header should exist")
        .to_str()
        .expect("signature header should be ascii")
        .to_owned();
    let body = resp.bytes().await.unwrap().to_vec();

    let public_key_bytes = qos_hex::decode(&public_key).expect("public key should hex decode");
    let public_key = P256Public::from_bytes(&public_key_bytes).expect("public key should decode");
    let signature = qos_hex::decode(&signature).expect("signature should hex decode");
    public_key
        .verify(&body, &signature)
        .expect("response signature should verify");

    body
}

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
        let body = verified_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
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
        json["time"]
            .as_str()
            .expect("time field should be a string")
            .parse::<u64>()
            .expect("time field should be a unix timestamp");
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_random_app_proof() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/random_app_proof", test_args.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();

        let random_number = json["payload"]["random_number"].as_str().unwrap();
        random_number.parse::<u64>().unwrap();
        let payload = json["proof"]["payload"].as_str().unwrap();
        let payload_json: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(
            payload_json,
            serde_json::json!({"random_number": random_number})
        );

        let public_key_bytes =
            qos_hex::decode(json["proof"]["public_key"].as_str().unwrap()).unwrap();
        let public_key = P256Public::from_bytes(&public_key_bytes).unwrap();
        let signature = qos_hex::decode(json["proof"]["signature"].as_str().unwrap()).unwrap();
        public_key.verify(payload.as_bytes(), &signature).unwrap();
    }
    e2e::Builder::new().execute(test).await;
}

#[tokio::test]
async fn test_quorum_key_encrypt_decrypt() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let plaintext = "hello TVC world";
        let resp = client
            .post(format!("{}/quorum_key/encrypt", test_args.base_url))
            .json(&serde_json::json!({ "plaintext": plaintext }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        let ciphertext = json["ciphertext"].as_str().unwrap();
        qos_hex::decode(ciphertext).unwrap();

        let resp = client
            .post(format!("{}/quorum_key/decrypt", test_args.base_url))
            .json(&serde_json::json!({ "ciphertext": ciphertext }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["plaintext"], plaintext);
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
        let body = verified_body(resp).await;
        assert_eq!(body, b"hello echo");
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
