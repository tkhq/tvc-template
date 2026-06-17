#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use e2e::TestArgs;
use qos_p256::P256Public;

const EPHEMERAL_SIGNATURE_HEADER: &str = "x-tvc-ephemeral-signature";
const QUORUM_SIGNATURE_HEADER: &str = "x-tvc-quorum-signature";
const SIGNATURE_TIMESTAMP_HEADER: &str = "x-tvc-signature-timestamp";

fn timestamped_signing_payload(body: &[u8], timestamp: &str) -> Vec<u8> {
    format!(r#"{{"body":"{}","timestamp":"{timestamp}"}}"#, qos_hex::encode(body)).into_bytes()
}

async fn verified_body(
    resp: reqwest::Response,
    ephemeral_public_key: &P256Public,
    quorum_public_key: &P256Public,
) -> Vec<u8> {
    let ephemeral_signature = resp
        .headers()
        .get(EPHEMERAL_SIGNATURE_HEADER)
        .expect("ephemeral signature header should exist")
        .to_str()
        .expect("ephemeral signature header should be ascii")
        .to_owned();
    let quorum_signature = resp
        .headers()
        .get(QUORUM_SIGNATURE_HEADER)
        .expect("quorum signature header should exist")
        .to_str()
        .expect("quorum signature header should be ascii")
        .to_owned();
    let timestamp = resp
        .headers()
        .get(SIGNATURE_TIMESTAMP_HEADER)
        .expect("timestamp header should exist")
        .to_str()
        .expect("timestamp header should be ascii")
        .to_owned();
    let body = resp.bytes().await.unwrap().to_vec();

    let payload = timestamped_signing_payload(&body, &timestamp);
    let ephemeral_signature =
        qos_hex::decode(&ephemeral_signature).expect("ephemeral signature should hex decode");
    let quorum_signature =
        qos_hex::decode(&quorum_signature).expect("quorum signature should hex decode");
    ephemeral_public_key
        .verify(&payload, &ephemeral_signature)
        .expect("ephemeral response signature should verify");
    quorum_public_key
        .verify(&payload, &quorum_signature)
        .expect("quorum response signature should verify");

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
        let body = verified_body(
            resp,
            &test_args.ephemeral_public_key,
            &test_args.quorum_public_key,
        )
        .await;
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
        let body = verified_body(
            resp,
            &test_args.ephemeral_public_key,
            &test_args.quorum_public_key,
        )
        .await;
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
