//! Router for the Hello World REST server
use crate::handlers::{
    btc_price, download, echo, health, hello_world, quorum_key_decrypt, quorum_key_encrypt,
    random_app_proof, time, turnkey_sign_transaction,
};
use axum::{
    Router,
    routing::{get, post},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub use crate::state::AppState;

/// Build the application router with the given state.
pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/btc_price", get(btc_price))
        .route("/download", get(download))
        .route("/random_app_proof", get(random_app_proof))
        .route("/quorum_key/encrypt", post(quorum_key_encrypt))
        .route("/quorum_key/decrypt", post(quorum_key_decrypt))
        .route("/turnkey/sign_transaction", post(turnkey_sign_transaction))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::StatusCode;
    use base64::{Engine as _, prelude::BASE64_URL_SAFE_NO_PAD};
    use http_body_util::BodyExt;
    use p256::ecdsa::{DerSignature, signature::Verifier as _};
    use qos_p256::{P256Pair, P256Public};
    use tower::ServiceExt;
    use turnkey_api_key_stamper::SIGNATURE_SCHEME_P256;

    async fn body_string(body: Body) -> String {
        let bytes = body
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
    }

    fn router_with_generated_keys() -> Router {
        router_with_generated_keys_and_quorum_public().0
    }

    fn router_with_generated_keys_and_quorum_public() -> (Router, P256Public) {
        let ephemeral_key = P256Pair::generate().expect("failed to generate ephemeral key");
        let quorum_key = P256Pair::generate().expect("failed to generate quorum key");
        let quorum_public = quorum_key.public_key();

        (
            router_with_state(
                AppState::new(ephemeral_key, quorum_key).expect("failed to build app state"),
            ),
            quorum_public,
        )
    }

    #[tokio::test]
    async fn turnkey_sign_transaction_returns_verifiable_quorum_stamp() {
        let (app, quorum_public) = router_with_generated_keys_and_quorum_public();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/turnkey/sign_transaction")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "organizationId": "organization-id",
                            "signWith": "0x1234",
                            "type": "TRANSACTION_TYPE_ETHEREUM",
                            "unsignedTransaction": "02f86c"
                        }"#,
                    ))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response.into_body()).await;
        let response_json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        let activity_body = response_json["activityBody"]
            .as_str()
            .expect("activityBody should be a string");
        let activity: serde_json::Value =
            serde_json::from_str(activity_body).expect("activityBody is not valid JSON");

        assert_eq!(activity["type"], "ACTIVITY_TYPE_SIGN_TRANSACTION_V2");
        assert!(
            activity["timestampMs"]
                .as_str()
                .expect("timestampMs should be a string")
                .parse::<u128>()
                .is_ok(),
            "timestampMs should be a decimal string"
        );
        assert_eq!(activity["organizationId"], "organization-id");
        assert_eq!(
            activity["parameters"],
            serde_json::json!({
                "signWith": "0x1234",
                "unsignedTransaction": "02f86c",
                "type": "TRANSACTION_TYPE_ETHEREUM"
            })
        );
        assert!(activity["generateAppProofs"].is_null());

        let stamp_bytes = BASE64_URL_SAFE_NO_PAD
            .decode(
                response_json["xStamp"]
                    .as_str()
                    .expect("xStamp should be a string"),
            )
            .expect("xStamp should be base64url without padding");
        let stamp: serde_json::Value =
            serde_json::from_slice(&stamp_bytes).expect("stamp should be valid JSON");
        let compressed_public = quorum_public
            .signing_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        assert_eq!(stamp["publicKey"], qos_hex::encode(&compressed_public));
        assert_eq!(stamp["scheme"], SIGNATURE_SCHEME_P256);

        let signature_bytes = qos_hex::decode(
            stamp["signature"]
                .as_str()
                .expect("signature should be a string"),
        )
        .expect("signature should be hex");
        let signature =
            DerSignature::from_bytes(&signature_bytes).expect("signature should be DER encoded");
        quorum_public
            .signing_key()
            .verify(activity_body.as_bytes(), &signature)
            .expect("quorum signature should verify over the exact activityBody");
    }

    #[tokio::test]
    async fn turnkey_sign_transaction_rejects_malformed_type() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/turnkey/sign_transaction")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "organizationId": "organization-id",
                            "signWith": "0x1234",
                            "type": "ETHEREUM",
                            "unsignedTransaction": "02f86c"
                        }"#,
                    ))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_health() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn test_hello_world() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/hello_world")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert_eq!(json["message"], "hello world");
    }

    #[tokio::test]
    async fn test_time() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/time")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert!(json["time"].is_u64(), "time field should be a number");
    }

    #[tokio::test]
    async fn random_app_proof() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/random_app_proof")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");

        let random_number = json["payload"]["random_number"]
            .as_u64()
            .expect("random_number should be a JSON number");
        let payload = json["proof"]["payload"]
            .as_str()
            .expect("proof payload should be a string");
        let payload_json: serde_json::Value =
            serde_json::from_str(payload).expect("payload is not valid JSON");
        assert_eq!(
            payload_json,
            serde_json::json!({"random_number": random_number.to_string()})
        );

        let public_key = P256Public::from_bytes(
            &qos_hex::decode(
                json["proof"]["public_key"]
                    .as_str()
                    .expect("public key should be a string"),
            )
            .expect("public key should hex decode"),
        )
        .expect("public key should decode");
        let signature = qos_hex::decode(
            json["proof"]["signature"]
                .as_str()
                .expect("signature should be a string"),
        )
        .expect("signature should hex decode");

        public_key
            .verify(payload.as_bytes(), &signature)
            .expect("proof signature should verify");
    }

    #[tokio::test]
    async fn test_echo_text() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/echo")
                    .body(Body::from("hello echo"))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        assert_eq!(body, "hello echo");
    }

    #[tokio::test]
    async fn test_echo_empty() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/echo")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        assert_eq!(body, "");
    }

    #[tokio::test]
    async fn test_echo_json() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/echo")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"foo":"bar"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), 200);
        let body = body_string(response.into_body()).await;
        assert_eq!(body, r#"{"foo":"bar"}"#);
    }

    #[tokio::test]
    async fn quorum_key_encrypt_and_decrypt_round_trip_utf8_payload() {
        let app = router_with_generated_keys();
        let plaintext = "hello TVC world";
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/quorum_key/encrypt")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"plaintext":"{plaintext}"}}"#)))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        let ciphertext = json["ciphertext"]
            .as_str()
            .expect("ciphertext should be a string");
        qos_hex::decode(ciphertext).expect("ciphertext should be hex");

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/quorum_key/decrypt")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"ciphertext":"{ciphertext}"}}"#)))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        assert_eq!(json["plaintext"], plaintext);
    }

    #[tokio::test]
    async fn quorum_key_decrypt_rejects_malformed_ciphertext_hex() {
        let app = router_with_generated_keys();
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/quorum_key/decrypt")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"ciphertext":"not-hex"}"#))
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
