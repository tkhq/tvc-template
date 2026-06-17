#![allow(missing_docs, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body_util::{BodyExt, Full};
use qos_p256::{P256Pair, P256Public};
use serde::Serialize;
use std::sync::Arc;
use tower::{ServiceBuilder, ServiceExt, service_fn};
use tvc_axum::{QosJson, ResponseSigningLayer};

const EPHEMERAL_SIGNATURE_HEADER: &str = "x-tvc-ephemeral-signature";
const QUORUM_SIGNATURE_HEADER: &str = "x-tvc-quorum-signature";
const SIGNATURE_TIMESTAMP_HEADER: &str = "x-tvc-signature-timestamp";

#[derive(Serialize)]
struct Sample {
    message: String,
    #[serde(with = "qos_json::string_or_numeric")]
    count: u64,
}

async fn body_bytes(response: Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes()
        .to_vec()
}

fn public_key(key: &P256Pair) -> P256Public {
    P256Public::from_bytes(&key.public_key().to_bytes()).expect("public key should decode")
}

fn signing_payload(body: &[u8]) -> Vec<u8> {
    format!(r#"{{"body":"{}"}}"#, qos_hex::encode(body)).into_bytes()
}

fn timestamped_signing_payload(body: &[u8], timestamp: &str) -> Vec<u8> {
    format!(r#"{{"body":"{}","timestamp":"{timestamp}"}}"#, qos_hex::encode(body)).into_bytes()
}

fn header_signature(response: &Response, header: &str) -> Vec<u8> {
    let signature = response
        .headers()
        .get(header)
        .expect("signature header should exist")
        .to_str()
        .expect("signature header should be ascii");
    qos_hex::decode(signature).expect("signature should be hex")
}

#[tokio::test]
async fn qos_json_body_matches_qos_json_to_vec() {
    let value = Sample {
        message: "hello".to_owned(),
        count: 42,
    };
    let expected = qos_json::to_vec(&value).expect("qos_json should serialize");

    let response = QosJson(value).into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static("application/json"))
    );
    assert_eq!(body_bytes(response).await, expected);
}

#[tokio::test]
async fn response_signing_builder_adds_only_configured_ephemeral_signature() {
    let key = Arc::new(P256Pair::generate().expect("key should generate"));
    let service = ServiceBuilder::new()
        .layer(
            ResponseSigningLayer::builder()
                .ephemeral_key(Arc::clone(&key))
                .build(),
        )
        .service(service_fn(|_request: Request<Body>| async {
            Ok::<_, std::convert::Infallible>(Response::new(Body::from("signed body")))
        }));

    let response = service
        .oneshot(Request::new(Body::empty()))
        .await
        .expect("response should succeed");

    assert!(response.headers().contains_key(EPHEMERAL_SIGNATURE_HEADER));
    assert!(!response.headers().contains_key(QUORUM_SIGNATURE_HEADER));
    assert!(!response.headers().contains_key(SIGNATURE_TIMESTAMP_HEADER));
    assert!(!response.headers().contains_key("x-tvc-ephemeral-public-key"));
    assert_eq!(body_bytes(response).await, b"signed body");
}

#[tokio::test]
async fn response_signing_builder_adds_configured_ephemeral_and_quorum_signatures() {
    let ephemeral_key = Arc::new(P256Pair::generate().expect("ephemeral key should generate"));
    let quorum_key = Arc::new(P256Pair::generate().expect("quorum key should generate"));
    let ephemeral_public_key = public_key(&ephemeral_key);
    let quorum_public_key = public_key(&quorum_key);
    let service = ServiceBuilder::new()
        .layer(
            ResponseSigningLayer::builder()
                .ephemeral_key(Arc::clone(&ephemeral_key))
                .quorum_key(Arc::clone(&quorum_key))
                .build(),
        )
        .service(service_fn(|_request: Request<Body>| async {
            let body = Full::from("exact bytes");
            Ok::<_, std::convert::Infallible>(Response::new(body))
        }));

    let response = service
        .oneshot(Request::new(Body::empty()))
        .await
        .expect("response should succeed");
    let ephemeral_signature = header_signature(&response, EPHEMERAL_SIGNATURE_HEADER);
    let quorum_signature = header_signature(&response, QUORUM_SIGNATURE_HEADER);
    let body = body_bytes(response).await;
    let payload = signing_payload(&body);

    ephemeral_public_key
        .verify(&payload, &ephemeral_signature)
        .expect("ephemeral signature should verify over signing payload");
    quorum_public_key
        .verify(&payload, &quorum_signature)
        .expect("quorum signature should verify over signing payload");
}

#[tokio::test]
async fn response_signing_builder_can_include_timestamp_in_signing_payload() {
    let key = Arc::new(P256Pair::generate().expect("key should generate"));
    let public_key = public_key(&key);
    let service = ServiceBuilder::new()
        .layer(
            ResponseSigningLayer::builder()
                .ephemeral_key(Arc::clone(&key))
                .include_timestamp(true)
                .build(),
        )
        .service(service_fn(|_request: Request<Body>| async {
            Ok::<_, std::convert::Infallible>(Response::new(Body::from("timed body")))
        }));

    let response = service
        .oneshot(Request::new(Body::empty()))
        .await
        .expect("response should succeed");
    let timestamp = response
        .headers()
        .get(SIGNATURE_TIMESTAMP_HEADER)
        .expect("timestamp header should exist")
        .to_str()
        .expect("timestamp header should be ascii")
        .to_owned();
    timestamp
        .parse::<u64>()
        .expect("timestamp should be a unix timestamp");
    let signature = header_signature(&response, EPHEMERAL_SIGNATURE_HEADER);
    let body = body_bytes(response).await;
    let payload = timestamped_signing_payload(&body, &timestamp);

    public_key
        .verify(&payload, &signature)
        .expect("signature should verify over timestamped signing payload");
}
