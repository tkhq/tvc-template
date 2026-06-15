//! Tower middleware that signs every HTTP response with the ephemeral
//! qos_p256 key.
//!
//! The layer buffers each response body, signs the exact body bytes with the
//! enclave's ephemeral key (the same source used by app-proof routes), and
//! attaches two hex-encoded headers without otherwise altering the response:
//!
//! - [`PUBLIC_KEY_HEADER`] (`x-tvc-ephemeral-public-key`): the ephemeral
//!   public key as `public_key().to_bytes()`.
//! - [`SIGNATURE_HEADER`] (`x-tvc-response-signature`): the signature over the
//!   response body bytes as produced by `P256Pair::sign`.
//!
//! Clients can verify a response by hex-decoding both headers and calling
//! `P256Public::from_bytes(public_key).verify(body_bytes, signature)`.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{HeaderValue, Request, Response, header::HeaderName};
use http_body_util::BodyExt;
use qos_core::handles::EphemeralKeyHandle;

/// Header carrying the hex-encoded ephemeral public key used to sign the
/// response body.
pub const PUBLIC_KEY_HEADER: HeaderName = HeaderName::from_static("x-tvc-ephemeral-public-key");

/// Header carrying the hex-encoded qos_p256 signature over the response body
/// bytes.
pub const SIGNATURE_HEADER: HeaderName = HeaderName::from_static("x-tvc-response-signature");

/// Tower layer that signs every response body with the ephemeral qos_p256 key.
///
/// Construct it with the same [`EphemeralKeyHandle`] used to build application
/// state so that the signing key matches the enclave's ephemeral key.
#[derive(Debug, Clone)]
pub struct ResponseSigningLayer {
    ephemeral_key_handle: EphemeralKeyHandle<String>,
}

impl ResponseSigningLayer {
    /// Create a new layer that signs responses using the given ephemeral key
    /// handle.
    #[must_use]
    pub fn new(ephemeral_key_handle: EphemeralKeyHandle<String>) -> Self {
        Self {
            ephemeral_key_handle,
        }
    }
}

impl<S> tower::Layer<S> for ResponseSigningLayer {
    type Service = ResponseSigningService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseSigningService {
            inner,
            ephemeral_key_handle: self.ephemeral_key_handle.clone(),
        }
    }
}

/// Tower service produced by [`ResponseSigningLayer`].
#[derive(Debug, Clone)]
pub struct ResponseSigningService<S> {
    inner: S,
    ephemeral_key_handle: EphemeralKeyHandle<String>,
}

impl<S, ReqBody> tower::Service<Request<ReqBody>> for ResponseSigningService<S>
where
    S: tower::Service<Request<ReqBody>, Response = Response<Body>> + Send,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let ephemeral_key_handle = self.ephemeral_key_handle.clone();
        let future = self.inner.call(request);

        Box::pin(async move {
            let response = future.await?;
            Ok(sign_response(response, &ephemeral_key_handle).await)
        })
    }
}

/// Buffer the response body, sign the exact bytes with the ephemeral key, and
/// reattach the unchanged body with the signature headers.
///
/// If the body cannot be collected or the ephemeral key cannot be loaded or
/// used, the response is returned without signature headers so that endpoint
/// behavior (status, body, content-type) is always preserved.
async fn sign_response(
    response: Response<Body>,
    ephemeral_key_handle: &EphemeralKeyHandle<String>,
) -> Response<Body> {
    let (mut parts, body) = response.into_parts();

    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(error) => {
            tracing::error!("failed to buffer response body for signing: {error}");
            return Response::from_parts(parts, Body::empty());
        }
    };

    match sign_bytes(&bytes, ephemeral_key_handle) {
        Ok((public_key_hex, signature_hex)) => {
            if let Ok(value) = HeaderValue::from_str(&public_key_hex) {
                parts.headers.insert(PUBLIC_KEY_HEADER, value);
            }
            if let Ok(value) = HeaderValue::from_str(&signature_hex) {
                parts.headers.insert(SIGNATURE_HEADER, value);
            }
        }
        Err(error) => {
            tracing::error!("failed to sign response body: {error}");
        }
    }

    Response::from_parts(parts, Body::from(bytes))
}

/// Sign the given bytes, returning the hex-encoded public key and signature.
fn sign_bytes(
    bytes: &[u8],
    ephemeral_key_handle: &EphemeralKeyHandle<String>,
) -> Result<(String, String), String> {
    let ephemeral_key = ephemeral_key_handle
        .get_ephemeral_key()
        .map_err(|e| format!("failed to load ephemeral key: {e:?}"))?;
    let signature = ephemeral_key
        .sign(bytes)
        .map_err(|e| format!("failed to sign response body: {e:?}"))?;
    let public_key_hex = qos_hex::encode(&ephemeral_key.public_key().to_bytes());
    let signature_hex = qos_hex::encode(&signature);
    Ok((public_key_hex, signature_hex))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::QosJson;
    use axum::{Router, http::Request, routing::get, routing::post};
    use qos_p256::{P256Pair, P256Public};
    use serde::Serialize;
    use tower::ServiceExt;

    #[derive(Serialize)]
    struct HealthResponse {
        status: String,
    }

    fn router_with_temp_key() -> (Router, tempfile::TempDir) {
        let ephemeral_key = P256Pair::generate().expect("failed to generate ephemeral key");
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let ephemeral_key_path = temp_dir.path().join("ephemeral.secret");

        ephemeral_key
            .to_hex_file(&ephemeral_key_path)
            .expect("failed to write ephemeral key");

        let app = Router::new()
            .route(
                "/health",
                get(|| async {
                    QosJson(HealthResponse {
                        status: "healthy".to_string(),
                    })
                }),
            )
            .route(
                "/echo",
                post(|body: Body| async move { Response::new(body) }),
            )
            .layer(ResponseSigningLayer::new(EphemeralKeyHandle::new(
                ephemeral_key_path
                    .to_str()
                    .expect("temp path should be utf8")
                    .to_string(),
            )));

        (app, temp_dir)
    }

    async fn send(
        app: Router,
        request: Request<Body>,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let response = app
            .oneshot(request)
            .await
            .expect("failed to execute request");
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes()
            .to_vec();
        (status, headers, body)
    }

    fn assert_signature_verifies(headers: &axum::http::HeaderMap, body: &[u8]) {
        let public_key_hex = headers
            .get(PUBLIC_KEY_HEADER)
            .expect("response should carry the ephemeral public key header")
            .to_str()
            .expect("public key header should be ascii");
        let signature_hex = headers
            .get(SIGNATURE_HEADER)
            .expect("response should carry the signature header")
            .to_str()
            .expect("signature header should be ascii");

        let public_key_bytes =
            qos_hex::decode(public_key_hex).expect("public key should hex decode");
        let signature = qos_hex::decode(signature_hex).expect("signature should hex decode");

        let public_key =
            P256Public::from_bytes(&public_key_bytes).expect("public key should decode");
        public_key
            .verify(body, &signature)
            .expect("signature should verify over the response body bytes");
    }

    #[tokio::test]
    async fn json_endpoint_body_preserved_and_signed() {
        let (app, _temp_dir) = router_with_temp_key();
        let (status, headers, body) = send(
            app,
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("failed to build request"),
        )
        .await;

        assert_eq!(status, 200);
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("response is not valid JSON");
        assert_eq!(json["status"], "healthy");
        assert_signature_verifies(&headers, &body);
    }

    #[tokio::test]
    async fn text_endpoint_body_preserved_and_signed() {
        let (app, _temp_dir) = router_with_temp_key();
        let payload = "hello signed echo";
        let (status, headers, body) = send(
            app,
            Request::builder()
                .method("POST")
                .uri("/echo")
                .body(Body::from(payload))
                .expect("failed to build request"),
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body, payload.as_bytes());
        assert_signature_verifies(&headers, &body);
    }

    #[tokio::test]
    async fn tampered_body_fails_verification() {
        let (app, _temp_dir) = router_with_temp_key();
        let (_status, headers, body) = send(
            app,
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("failed to build request"),
        )
        .await;

        let public_key_bytes = qos_hex::decode(
            headers
                .get(PUBLIC_KEY_HEADER)
                .expect("public key header")
                .to_str()
                .expect("ascii"),
        )
        .expect("hex");
        let signature = qos_hex::decode(
            headers
                .get(SIGNATURE_HEADER)
                .expect("signature header")
                .to_str()
                .expect("ascii"),
        )
        .expect("hex");
        let public_key = P256Public::from_bytes(&public_key_bytes).expect("decode");

        let mut tampered = body.clone();
        tampered.push(b'!');
        assert!(
            public_key.verify(&tampered, &signature).is_err(),
            "signature must not verify over a modified body"
        );
    }
}
