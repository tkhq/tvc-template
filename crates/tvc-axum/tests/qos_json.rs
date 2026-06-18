#![allow(missing_docs, clippy::expect_used, clippy::panic)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::{BodyExt, Full};
use qos_p256::{P256Pair, P256Public};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::{ServiceBuilder, ServiceExt, service_fn};
use tvc_axum::{QosJson, ResponseSigningLayer};

const SIGNATURE_COMPONENTS: &str = "(\"@method\" \"@path\" \"@status\" \"content-digest\")";
const SIGNATURE_ALG: &str = "ecdsa-p256-sha256";

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

fn content_digest(body: &[u8]) -> String {
    format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(body)))
}

fn signature_input(label: &str, created: u64) -> String {
    format!(r#"{SIGNATURE_COMPONENTS};created={created};keyid="{label}";alg="{SIGNATURE_ALG}""#)
}

fn signature_base(
    method: &str,
    path: &str,
    status: StatusCode,
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

fn header_str<'a>(response: &'a Response, name: &str) -> &'a str {
    response
        .headers()
        .get(name)
        .unwrap_or_else(|| panic!("{name} header should exist"))
        .to_str()
        .unwrap_or_else(|_| panic!("{name} header should be ascii"))
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

fn assert_rfc_headers_absent(response: &Response) {
    assert!(!response.headers().contains_key("x-tvc-ephemeral-signature"));
    assert!(!response.headers().contains_key("x-tvc-quorum-signature"));
    assert!(!response.headers().contains_key("x-tvc-signature-timestamp"));
    assert!(
        !response
            .headers()
            .contains_key("x-tvc-ephemeral-public-key")
    );
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
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/signed")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("response should succeed");

    assert!(response.headers().contains_key("content-digest"));
    assert!(response.headers().contains_key("signature-input"));
    assert!(response.headers().contains_key("signature"));
    assert!(!header_str(&response, "signature-input").contains("quorum="));
    assert!(!header_str(&response, "signature").contains("quorum="));
    assert_rfc_headers_absent(&response);
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
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/exact?ignored=true")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("response should succeed");
    let digest = header_str(&response, "content-digest").to_owned();
    let signature_input_header = header_str(&response, "signature-input").to_owned();
    let signature_header = header_str(&response, "signature").to_owned();
    let created = created_from_signature_input(&signature_input_header, "ephemeral");
    assert_eq!(
        created_from_signature_input(&signature_input_header, "quorum"),
        created
    );
    let body = body_bytes(response).await;
    assert_eq!(digest, content_digest(&body));
    assert_eq!(
        signature_input_header,
        format!(
            "ephemeral={}, quorum={}",
            signature_input("ephemeral", created),
            signature_input("quorum", created)
        )
    );

    ephemeral_public_key
        .verify(
            &signature_base(
                "GET",
                "/exact",
                StatusCode::OK,
                &digest,
                "ephemeral",
                created,
            ),
            &signature_bytes(&signature_header, "ephemeral"),
        )
        .expect("ephemeral signature should verify over signing payload");
    quorum_public_key
        .verify(
            &signature_base("GET", "/exact", StatusCode::OK, &digest, "quorum", created),
            &signature_bytes(&signature_header, "quorum"),
        )
        .expect("quorum signature should verify over signing payload");
}

#[tokio::test]
async fn response_signature_fails_when_signed_components_are_tampered() {
    let key = Arc::new(P256Pair::generate().expect("key should generate"));
    let public_key = public_key(&key);
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
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tamper")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("response should succeed");
    let digest = header_str(&response, "content-digest").to_owned();
    let signature_input_header = header_str(&response, "signature-input").to_owned();
    let signature_header = header_str(&response, "signature").to_owned();
    let created = created_from_signature_input(&signature_input_header, "ephemeral");
    let body = body_bytes(response).await;
    let signature = signature_bytes(&signature_header, "ephemeral");

    public_key
        .verify(
            &signature_base(
                "POST",
                "/tamper",
                StatusCode::OK,
                &digest,
                "ephemeral",
                created,
            ),
            &signature,
        )
        .expect("signature should verify over expected signing payload");
    for tampered_base in [
        signature_base(
            "GET",
            "/tamper",
            StatusCode::OK,
            &digest,
            "ephemeral",
            created,
        ),
        signature_base(
            "POST",
            "/other",
            StatusCode::OK,
            &digest,
            "ephemeral",
            created,
        ),
        signature_base(
            "POST",
            "/tamper",
            StatusCode::NOT_FOUND,
            &digest,
            "ephemeral",
            created,
        ),
        signature_base(
            "POST",
            "/tamper",
            StatusCode::OK,
            &content_digest(b"tampered body"),
            "ephemeral",
            created,
        ),
        signature_base(
            "POST",
            "/tamper",
            StatusCode::OK,
            &content_digest(&body),
            "ephemeral",
            created + 1,
        ),
    ] {
        assert!(
            public_key.verify(&tampered_base, &signature).is_err(),
            "tampered signature base should fail verification"
        );
    }
}
