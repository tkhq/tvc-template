#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use e2e::TestArgs;
use qos_core::protocol::services::boot::VersionedManifestEnvelope;
use qos_nsm::mock::{MOCK_ROOT_CERT_DER, MOCK_SECONDS_SINCE_EPOCH};
use qos_nsm::nitro;
use qos_p256::P256Public;
use sha2::{Digest, Sha256};
use tvc_axum::{ATTESTATION_DOC_HEADER, MANIFEST_ENVELOPE_HEADER};

const SIGNATURE_COMPONENTS: &str = "(\"@method\" \"@path\" \"@status\" \"content-digest\")";
const SIGNATURE_ALG: &str = "ecdsa-p256-sha256";

fn content_digest(body: &[u8]) -> String {
    format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(body)))
}

fn signature_input(label: &str, created: u64) -> String {
    format!(r#"{SIGNATURE_COMPONENTS};created={created};keyid="{label}";alg="{SIGNATURE_ALG}""#)
}

fn signature_base(
    method: &str,
    path: &str,
    status: reqwest::StatusCode,
    digest: &str,
    label: &str,
    created: u64,
) -> Vec<u8> {
    format!(
        "\"@method\": {method}\n\"@path\": {path}\n\"@status\": {}\n\"content-digest\": {digest}\n\"@signature-params\": {}",
        status.as_u16(),
        signature_input(label, created)
    )
    .into_bytes()
}

fn label_value<'a>(header: &'a str, label: &str) -> &'a str {
    header
        .split(", ")
        .find_map(|value| value.strip_prefix(&format!("{label}=")))
        .unwrap_or_else(|| panic!("{label} value should exist"))
}

fn created_from_signature_input(input: &str, label: &str) -> u64 {
    let value = label_value(input, label);
    let created = value
        .strip_prefix(&format!(r#"{SIGNATURE_COMPONENTS};created="#))
        .and_then(|value| value.split_once(';').map(|(created, _)| created))
        .expect("created parameter should exist");
    created.parse().expect("created should be a unix timestamp")
}

fn signature_bytes(signature_header: &str, label: &str) -> Vec<u8> {
    let signature = label_value(signature_header, label)
        .strip_prefix(':')
        .and_then(|value| value.strip_suffix(':'))
        .expect("signature should be an RFC byte sequence");
    STANDARD
        .decode(signature)
        .expect("signature should be base64")
}

async fn verified_body(
    resp: reqwest::Response,
    method: &str,
    path: &str,
    ephemeral_public_key: &P256Public,
    quorum_public_key: &P256Public,
) -> Vec<u8> {
    let status = resp.status();
    let digest = resp
        .headers()
        .get("content-digest")
        .expect("content-digest header should exist")
        .to_str()
        .expect("content-digest header should be ascii")
        .to_owned();
    let signature_input_header = resp
        .headers()
        .get("signature-input")
        .expect("signature-input header should exist")
        .to_str()
        .expect("signature-input header should be ascii")
        .to_owned();
    let signature_header = resp
        .headers()
        .get("signature")
        .expect("signature header should exist")
        .to_str()
        .expect("signature header should be ascii")
        .to_owned();
    assert!(!resp.headers().contains_key("x-tvc-ephemeral-signature"));
    assert!(!resp.headers().contains_key("x-tvc-quorum-signature"));
    assert!(!resp.headers().contains_key("x-tvc-signature-timestamp"));
    let created = created_from_signature_input(&signature_input_header, "ephemeral");
    assert_eq!(
        created_from_signature_input(&signature_input_header, "quorum"),
        created
    );
    let body = resp.bytes().await.unwrap().to_vec();

    assert_eq!(digest, content_digest(&body));
    ephemeral_public_key
        .verify(
            &signature_base(method, path, status, &digest, "ephemeral", created),
            &signature_bytes(&signature_header, "ephemeral"),
        )
        .expect("ephemeral response signature should verify");
    quorum_public_key
        .verify(
            &signature_base(method, path, status, &digest, "quorum", created),
            &signature_bytes(&signature_header, "quorum"),
        )
        .expect("quorum response signature should verify");

    body
}

fn header_str(resp: &reqwest::Response, name: &str) -> String {
    resp.headers()
        .get(name)
        .unwrap_or_else(|| panic!("{name} header should exist"))
        .to_str()
        .unwrap_or_else(|_| panic!("{name} header should be ascii"))
        .to_owned()
}

/// Verify the full trust chain for a response: manifest envelope ->
/// attestation document -> response signature. The ephemeral public key is
/// taken from the attestation document, never from the harness.
#[tokio::test]
async fn test_full_trust_chain() {
    async fn test(test_args: TestArgs) {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/hello_world", test_args.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // 1. The manifest envelope header carries the exact canonical bytes
        // the server booted with.
        let manifest_bytes = STANDARD
            .decode(header_str(&resp, MANIFEST_ENVELOPE_HEADER))
            .expect("manifest header should be base64");
        assert_eq!(
            manifest_bytes,
            test_args
                .manifest_envelope
                .to_storage_vec()
                .expect("manifest envelope should serialize")
        );
        let envelope = VersionedManifestEnvelope::try_from_slice_compat(&manifest_bytes)
            .expect("manifest header should decode");

        // 2. The attestation document verifies up to the NSM root and binds
        // the manifest hash as user_data.
        let attestation_bytes = STANDARD
            .decode(header_str(&resp, ATTESTATION_DOC_HEADER))
            .expect("attestation header should be base64");
        let attestation_doc = nitro::attestation_doc_from_der(
            &attestation_bytes,
            MOCK_ROOT_CERT_DER,
            MOCK_SECONDS_SINCE_EPOCH,
        )
        .expect("attestation doc should verify against the NSM root");
        assert_eq!(
            attestation_doc.user_data.as_deref().map(Vec::as_slice),
            Some(envelope.manifest_hash().as_slice()),
            "attestation user_data should be the manifest hash"
        );

        // 3. The ephemeral public key extracted from the attestation
        // document verifies the response signature.
        let attested_key_bytes = attestation_doc
            .public_key
            .expect("attestation doc should bind the ephemeral public key")
            .into_vec();
        let attested_key =
            P256Public::from_bytes(&attested_key_bytes).expect("attested key should decode");

        let status = resp.status();
        let digest = header_str(&resp, "content-digest");
        let signature_input_header = header_str(&resp, "signature-input");
        let signature_header = header_str(&resp, "signature");
        let created = created_from_signature_input(&signature_input_header, "ephemeral");
        let body = resp.bytes().await.unwrap().to_vec();
        assert_eq!(digest, content_digest(&body));

        attested_key
            .verify(
                &signature_base("GET", "/hello_world", status, &digest, "ephemeral", created),
                &signature_bytes(&signature_header, "ephemeral"),
            )
            .expect("ephemeral signature should verify with the attested key");
    }
    e2e::Builder::new().execute(test).await;
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
            "GET",
            "/health",
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
            "POST",
            "/echo",
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
