//! Axum adapters for Turnkey Verifiable Cloud applications.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes, HttpBody};
use axum::http::{HeaderValue, Request, Response, StatusCode, header};
use axum::response::IntoResponse;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::BodyExt;
use qos_p256::P256Pair;
use serde::Serialize;
use sha2::{Digest, Sha256};

const SIGNATURE_COMPONENTS: &str = "(\"@method\" \"@path\" \"@status\" \"content-digest\")";
const SIGNATURE_ALG: &str = "ecdsa-p256-sha256";

fn unix_timestamp() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .ok()
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

fn signature_value(key: &P256Pair, signature_base: &[u8]) -> Option<String> {
    let signature = key.sign(signature_base).ok()?;
    Some(format!(":{}:", STANDARD.encode(signature)))
}

fn internal_error_response(message: &'static str) -> Response<Body> {
    let mut response = Response::new(Body::from(format!(r#"{{"error":"{message}"}}"#)));
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

/// Axum response adapter that serializes response bodies with `qos_json`.
pub struct QosJson<T>(pub T);

impl<T> IntoResponse for QosJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> axum::response::Response {
        match qos_json::to_vec(&self.0) {
            Ok(bytes) => {
                let mut response = Response::new(Body::from(bytes));
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                response
            }
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "application/json")],
                r#"{"error":"serialization failed"}"#,
            )
                .into_response(),
        }
    }
}

/// Tower layer that signs response bodies with configured TVC P-256 keys.
#[derive(Clone)]
pub struct ResponseSigningLayer {
    ephemeral_key: Option<Arc<P256Pair>>,
    quorum_key: Option<Arc<P256Pair>>,
}

impl ResponseSigningLayer {
    /// Create a builder for response signing middleware.
    #[must_use]
    pub fn builder() -> ResponseSigningLayerBuilder {
        ResponseSigningLayerBuilder::default()
    }
}

/// Builder for [`ResponseSigningLayer`].
#[derive(Default)]
pub struct ResponseSigningLayerBuilder {
    ephemeral_key: Option<Arc<P256Pair>>,
    quorum_key: Option<Arc<P256Pair>>,
}

impl ResponseSigningLayerBuilder {
    /// Sign responses with the TVC ephemeral key.
    #[must_use]
    pub fn ephemeral_key(mut self, key: Arc<P256Pair>) -> Self {
        self.ephemeral_key = Some(key);
        self
    }

    /// Sign responses with the TVC quorum key.
    #[must_use]
    pub fn quorum_key(mut self, key: Arc<P256Pair>) -> Self {
        self.quorum_key = Some(key);
        self
    }

    /// Build the response signing layer.
    #[must_use]
    pub fn build(self) -> ResponseSigningLayer {
        ResponseSigningLayer {
            ephemeral_key: self.ephemeral_key,
            quorum_key: self.quorum_key,
        }
    }
}

impl<S> tower::Layer<S> for ResponseSigningLayer {
    type Service = ResponseSigningService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseSigningService {
            inner,
            ephemeral_key: self.ephemeral_key.clone(),
            quorum_key: self.quorum_key.clone(),
        }
    }
}

/// Tower service produced by [`ResponseSigningLayer`].
#[derive(Clone)]
pub struct ResponseSigningService<S> {
    inner: S,
    ephemeral_key: Option<Arc<P256Pair>>,
    quorum_key: Option<Arc<P256Pair>>,
}

impl<S, ReqBody, ResBody> tower::Service<Request<ReqBody>> for ResponseSigningService<S>
where
    S: tower::Service<Request<ReqBody>, Response = Response<ResBody>>,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: HttpBody<Data = Bytes> + Send + 'static,
    ResBody::Error: std::fmt::Display,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let method = request.method().as_str().to_owned();
        let path = request.uri().path().to_owned();
        let future = self.inner.call(request);
        let ephemeral_key = self.ephemeral_key.clone();
        let quorum_key = self.quorum_key.clone();

        Box::pin(async move {
            let response = future.await?;
            let (mut parts, body) = response.into_parts();
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => return Ok(internal_error_response("failed to read response body")),
            };

            let created = match unix_timestamp() {
                Some(created) => created,
                None => return Ok(internal_error_response("failed to read system time")),
            };
            let digest = content_digest(&body_bytes);
            let mut signature_inputs = Vec::new();
            let mut signatures = Vec::new();

            if let Some(ephemeral_key) = ephemeral_key {
                let signature_input = signature_input("ephemeral", created);
                let signature_base =
                    signature_base(&method, &path, parts.status, &digest, "ephemeral", created);
                let signature = match signature_value(&ephemeral_key, &signature_base) {
                    Some(signature) => signature,
                    None => {
                        return Ok(internal_error_response(
                            "failed to sign response with ephemeral key",
                        ));
                    }
                };
                signature_inputs.push(format!("ephemeral={signature_input}"));
                signatures.push(format!("ephemeral={signature}"));
            }

            if let Some(quorum_key) = quorum_key {
                let signature_input = signature_input("quorum", created);
                let signature_base =
                    signature_base(&method, &path, parts.status, &digest, "quorum", created);
                let signature = match signature_value(&quorum_key, &signature_base) {
                    Some(signature) => signature,
                    None => {
                        return Ok(internal_error_response(
                            "failed to sign response with quorum key",
                        ));
                    }
                };
                signature_inputs.push(format!("quorum={signature_input}"));
                signatures.push(format!("quorum={signature}"));
            }

            let digest = match HeaderValue::from_str(&digest) {
                Ok(digest) => digest,
                Err(_) => return Ok(internal_error_response("failed to encode content digest")),
            };
            parts.headers.insert("content-digest", digest);

            if !signature_inputs.is_empty() {
                let signature_input = match HeaderValue::from_str(&signature_inputs.join(", ")) {
                    Ok(signature_input) => signature_input,
                    Err(_) => {
                        return Ok(internal_error_response("failed to encode signature input"));
                    }
                };
                let signature = match HeaderValue::from_str(&signatures.join(", ")) {
                    Ok(signature) => signature,
                    Err(_) => return Ok(internal_error_response("failed to encode signature")),
                };
                parts.headers.insert("signature-input", signature_input);
                parts.headers.insert("signature", signature);
            }

            Ok(Response::from_parts(parts, Body::from(body_bytes)))
        })
    }
}
