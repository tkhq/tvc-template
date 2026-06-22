//! Router for the Hello World REST server
use crate::handlers::{
    attestation, echo, health, hello_world, quorum_key_decrypt, quorum_key_encrypt,
    random_app_proof, time,
};
use axum::{
    Router,
    routing::{get, post},
};
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub use crate::state::AppState;

/// Build the application router with all routes.
pub fn router() -> Router {
    router_with_state(AppState::default())
}

/// Build the application router with the given state.
pub fn router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/hello_world", get(hello_world))
        .route("/time", get(time))
        .route("/echo", post(echo))
        .route("/attestation", get(attestation))
        .route("/random_app_proof", get(random_app_proof))
        .route("/quorum_key/encrypt", post(quorum_key_encrypt))
        .route("/quorum_key/decrypt", post(quorum_key_decrypt))
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
    use http_body_util::BodyExt;
    use qos_core::handles::{EphemeralKeyHandle, QuorumKeyHandle};
    use qos_core::protocol::services::boot::{
        Manifest, ManifestEnvelope, ManifestSet, Namespace, NitroConfig, PatchSet, PivotConfig,
        RestartPolicy, ShareSet, VersionedManifestEnvelope,
    };
    use qos_nsm::mock::{
        DynamicMockNsm, MOCK_PCR0, MOCK_PCR1, MOCK_PCR2, MOCK_PCR3, MOCK_SECONDS_SINCE_EPOCH,
        mock_root_certificate_der,
    };
    use qos_nsm::nitro::{
        AWS_ROOT_CERT_PEM, attestation_doc_from_der, cert_from_pem, unsafe_attestation_doc_from_der,
    };
    use qos_p256::{P256Pair, P256Public};
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn body_string(body: Body) -> String {
        let bytes = body
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        String::from_utf8(bytes.to_vec()).expect("invalid utf8")
    }

    fn sample_manifest_envelope(quorum_key: &P256Pair) -> VersionedManifestEnvelope {
        VersionedManifestEnvelope::V1(ManifestEnvelope {
            manifest: Manifest {
                namespace: Namespace {
                    name: "test-namespace".to_string(),
                    nonce: 1,
                    quorum_key: quorum_key.public_key().to_bytes(),
                },
                pivot: PivotConfig {
                    hash: [9; 32],
                    restart: RestartPolicy::Never,
                    bridge_config: vec![],
                    debug_mode: false,
                    args: vec![],
                },
                manifest_set: ManifestSet {
                    threshold: 0,
                    members: vec![],
                },
                share_set: ShareSet {
                    threshold: 0,
                    members: vec![],
                },
                enclave: NitroConfig {
                    pcr0: qos_hex::decode(MOCK_PCR0).expect("mock PCR0 should be hex"),
                    pcr1: qos_hex::decode(MOCK_PCR1).expect("mock PCR1 should be hex"),
                    pcr2: qos_hex::decode(MOCK_PCR2).expect("mock PCR2 should be hex"),
                    pcr3: qos_hex::decode(MOCK_PCR3).expect("mock PCR3 should be hex"),
                    aws_root_certificate: mock_root_certificate_der().to_vec(),
                    qos_commit: "32747120".to_string(),
                },
                patch_set: PatchSet {
                    threshold: 0,
                    members: vec![],
                },
            },
            manifest_set_approvals: vec![],
            share_set_approvals: vec![],
        })
    }

    fn router_with_temp_state(
        nsm_provider: Arc<dyn qos_nsm::NsmProvider>,
    ) -> (Router, tempfile::TempDir, P256Pair) {
        let ephemeral_key = P256Pair::generate().expect("failed to generate ephemeral key");
        let quorum_key = P256Pair::generate().expect("failed to generate quorum key");
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let ephemeral_key_path = temp_dir.path().join("ephemeral.secret");
        let quorum_key_path = temp_dir.path().join("quorum.secret");
        let manifest_path = temp_dir.path().join("qos.manifest");

        ephemeral_key
            .to_hex_file(&ephemeral_key_path)
            .expect("failed to write ephemeral key");
        quorum_key
            .to_hex_file(&quorum_key_path)
            .expect("failed to write quorum key");
        let manifest_envelope = sample_manifest_envelope(&quorum_key);
        std::fs::write(
            &manifest_path,
            manifest_envelope
                .to_storage_vec()
                .expect("manifest envelope should serialize"),
        )
        .expect("failed to write manifest");

        let app = router_with_state(AppState::new(
            EphemeralKeyHandle::new(
                ephemeral_key_path
                    .to_str()
                    .expect("temp path should be utf8")
                    .to_string(),
            ),
            QuorumKeyHandle::new(
                quorum_key_path
                    .to_str()
                    .expect("temp path should be utf8")
                    .to_string(),
            ),
            manifest_path
                .to_str()
                .expect("temp path should be utf8")
                .to_string(),
            nsm_provider,
        ));

        (app, temp_dir, ephemeral_key)
    }

    fn router_with_temp_keys() -> (Router, tempfile::TempDir) {
        let (app, temp_dir, _ephemeral_key) =
            router_with_temp_state(Arc::new(DynamicMockNsm::new()));
        (app, temp_dir)
    }

    #[tokio::test]
    async fn test_health() {
        let app = router();
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
        let app = router();
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
        let app = router();
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
        let (app, _temp_dir) = router_with_temp_keys();
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
    async fn returns_manifest_and_dynamic_attestation_doc() {
        let (app, _temp_dir, ephemeral_key) =
            router_with_temp_state(Arc::new(DynamicMockNsm::new()));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/attestation")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        let manifest_hash = qos_hex::decode(
            json["manifestEnvelope"]["manifestHash"]
                .as_str()
                .expect("manifest hash should be a string"),
        )
        .expect("manifest hash should hex decode");
        let attestation_doc = qos_hex::decode(
            json["attestationDoc"]
                .as_str()
                .expect("attestation doc should be a string"),
        )
        .expect("attestation doc should hex decode");

        assert!(json["manifestEnvelope"]["manifest"].is_object());
        assert!(!attestation_doc.is_empty());
        let doc = unsafe_attestation_doc_from_der(&attestation_doc)
            .expect("attestation doc should decode");
        assert_eq!(
            doc.user_data.as_ref().map(|data| data.as_slice()),
            Some(manifest_hash.as_slice())
        );
        assert_eq!(
            doc.public_key
                .as_ref()
                .map(|public_key| public_key.as_slice()),
            Some(ephemeral_key.public_key().to_bytes().as_slice())
        );
    }

    #[tokio::test]
    async fn verifies_mock_attestation_doc_against_mock_root() {
        let (app, _temp_dir, _ephemeral_key) = router_with_temp_state(Arc::new(
            DynamicMockNsm::new().with_mock_certificate_chain(),
        ));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/attestation")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to execute request");

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_string(response.into_body()).await;
        let json: serde_json::Value =
            serde_json::from_str(&body).expect("response is not valid JSON");
        let manifest_hash = qos_hex::decode(
            json["manifestEnvelope"]["manifestHash"]
                .as_str()
                .expect("manifest hash should be a string"),
        )
        .expect("manifest hash should hex decode");
        let attestation_doc = qos_hex::decode(
            json["attestationDoc"]
                .as_str()
                .expect("attestation doc should be a string"),
        )
        .expect("attestation doc should hex decode");

        let doc = attestation_doc_from_der(
            &attestation_doc,
            mock_root_certificate_der(),
            MOCK_SECONDS_SINCE_EPOCH,
        )
        .expect("attestation doc should verify against mock root");
        assert_eq!(
            doc.user_data.as_ref().map(|data| data.as_slice()),
            Some(manifest_hash.as_slice())
        );
        for (index, expected) in [
            (0, MOCK_PCR0),
            (1, MOCK_PCR1),
            (2, MOCK_PCR2),
            (3, MOCK_PCR3),
        ] {
            let expected = qos_hex::decode(expected).expect("mock PCR should be hex");
            assert_eq!(
                doc.pcrs.get(&index).map(AsRef::as_ref),
                Some(expected.as_slice())
            );
        }

        let aws_root = cert_from_pem(AWS_ROOT_CERT_PEM).expect("AWS root should decode");
        assert!(
            attestation_doc_from_der(&attestation_doc, &aws_root, MOCK_SECONDS_SINCE_EPOCH,)
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_echo_text() {
        let app = router();
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
        let app = router();
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
        let app = router();
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
        let (app, _temp_dir) = router_with_temp_keys();
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
        let (app, _temp_dir) = router_with_temp_keys();
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
