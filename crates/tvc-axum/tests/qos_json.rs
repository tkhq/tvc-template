#![allow(missing_docs, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body_util::{BodyExt, Full};
use qos_core::handles::EphemeralKeyHandle;
use qos_p256::{P256Pair, P256Public};
use serde::Serialize;
use tower::{ServiceBuilder, ServiceExt, service_fn};
use tvc_axum::{QosJson, ResponseSigningLayer};

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

fn temp_ephemeral_handle() -> (EphemeralKeyHandle<String>, tempfile::TempDir) {
    let key = P256Pair::generate().expect("key should generate");
    let temp_dir = tempfile::tempdir().expect("temp dir should create");
    let key_path = temp_dir.path().join("ephemeral.secret");
    key.to_hex_file(&key_path).expect("key should write");
    let path = key_path.to_str().expect("path should be utf8").to_owned();

    (EphemeralKeyHandle::new(path), temp_dir)
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
async fn response_signing_layer_adds_signature_headers() {
    let (handle, _temp_dir) = temp_ephemeral_handle();
    let service = ServiceBuilder::new()
        .layer(ResponseSigningLayer::new(handle))
        .service(service_fn(|_request: Request<Body>| async {
            Ok::<_, std::convert::Infallible>(Response::new(Body::from("signed body")))
        }));

    let response = service
        .oneshot(Request::new(Body::empty()))
        .await
        .expect("response should succeed");

    assert!(
        response
            .headers()
            .contains_key("x-tvc-ephemeral-public-key")
    );
    assert!(response.headers().contains_key("x-tvc-response-signature"));
    assert_eq!(body_bytes(response).await, b"signed body");
}

#[tokio::test]
async fn response_signature_verifies_over_exact_body_bytes() {
    let (handle, _temp_dir) = temp_ephemeral_handle();
    let service = ServiceBuilder::new()
        .layer(ResponseSigningLayer::new(handle))
        .service(service_fn(|_request: Request<Body>| async {
            let body = Full::from("exact bytes");
            Ok::<_, std::convert::Infallible>(Response::new(body))
        }));

    let response = service
        .oneshot(Request::new(Body::empty()))
        .await
        .expect("response should succeed");
    let public_key = response
        .headers()
        .get("x-tvc-ephemeral-public-key")
        .expect("public key header should exist")
        .to_str()
        .expect("public key header should be ascii")
        .to_owned();
    let signature = response
        .headers()
        .get("x-tvc-response-signature")
        .expect("signature header should exist")
        .to_str()
        .expect("signature header should be ascii")
        .to_owned();
    let body = body_bytes(response).await;

    let public_key_bytes = qos_hex::decode(&public_key).expect("public key should be hex");
    let public_key = P256Public::from_bytes(&public_key_bytes).expect("public key should decode");
    let signature = qos_hex::decode(&signature).expect("signature should be hex");

    public_key
        .verify(&body, &signature)
        .expect("signature should verify over exact body bytes");
}
