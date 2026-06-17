//! Axum adapters for Turnkey Verifiable Cloud applications.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes, HttpBody};
use axum::http::{HeaderValue, Request, Response, StatusCode, header};
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use qos_p256::P256Pair;
use serde::Serialize;

const EPHEMERAL_SIGNATURE_HEADER: &str = "x-tvc-ephemeral-signature";
const QUORUM_SIGNATURE_HEADER: &str = "x-tvc-quorum-signature";
const SIGNATURE_TIMESTAMP_HEADER: &str = "x-tvc-signature-timestamp";

#[derive(Serialize)]
struct ResponseSigningPayload {
    #[serde(with = "qos_hex::serde")]
    body: Vec<u8>,
}

#[derive(Serialize)]
struct TimestampedResponseSigningPayload {
    #[serde(with = "qos_hex::serde")]
    body: Vec<u8>,
    #[serde(with = "qos_json::string_or_numeric")]
    timestamp: u64,
}

fn signing_payload(body: &[u8], timestamp: Option<u64>) -> Option<Vec<u8>> {
    match timestamp {
        Some(timestamp) => qos_json::to_vec(&TimestampedResponseSigningPayload {
            body: body.to_vec(),
            timestamp,
        }),
        None => qos_json::to_vec(&ResponseSigningPayload {
            body: body.to_vec(),
        }),
    }
    .ok()
}

fn unix_timestamp() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .ok()
}

fn signature_header_value(key: &P256Pair, payload: &[u8]) -> Option<HeaderValue> {
    let signature = key.sign(payload).ok()?;
    HeaderValue::from_str(&qos_hex::encode(&signature)).ok()
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
    include_timestamp: bool,
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
    include_timestamp: bool,
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

    /// Include a Unix UTC timestamp in the signing payload and response headers.
    #[must_use]
    pub fn include_timestamp(mut self, include_timestamp: bool) -> Self {
        self.include_timestamp = include_timestamp;
        self
    }

    /// Build the response signing layer.
    #[must_use]
    pub fn build(self) -> ResponseSigningLayer {
        ResponseSigningLayer {
            ephemeral_key: self.ephemeral_key,
            quorum_key: self.quorum_key,
            include_timestamp: self.include_timestamp,
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
            include_timestamp: self.include_timestamp,
        }
    }
}

/// Tower service produced by [`ResponseSigningLayer`].
#[derive(Clone)]
pub struct ResponseSigningService<S> {
    inner: S,
    ephemeral_key: Option<Arc<P256Pair>>,
    quorum_key: Option<Arc<P256Pair>>,
    include_timestamp: bool,
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
        let future = self.inner.call(request);
        let ephemeral_key = self.ephemeral_key.clone();
        let quorum_key = self.quorum_key.clone();
        let include_timestamp = self.include_timestamp;

        Box::pin(async move {
            let response = future.await?;
            let (mut parts, body) = response.into_parts();
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => return Ok(internal_error_response("failed to read response body")),
            };

            let timestamp = if include_timestamp {
                match unix_timestamp() {
                    Some(timestamp) => Some(timestamp),
                    None => return Ok(internal_error_response("failed to read system time")),
                }
            } else {
                None
            };

            let signing_payload = match signing_payload(&body_bytes, timestamp) {
                Some(signing_payload) => signing_payload,
                None => return Ok(internal_error_response("failed to serialize signing payload")),
            };

            if let Some(timestamp) = timestamp {
                let timestamp = match HeaderValue::from_str(&timestamp.to_string()) {
                    Ok(timestamp) => timestamp,
                    Err(_) => {
                        return Ok(internal_error_response("failed to encode timestamp header"));
                    }
                };
                parts.headers.insert(SIGNATURE_TIMESTAMP_HEADER, timestamp);
            }

            if let Some(ephemeral_key) = ephemeral_key {
                let signature = match signature_header_value(&ephemeral_key, &signing_payload) {
                    Some(signature) => signature,
                    None => {
                        return Ok(internal_error_response(
                            "failed to sign response with ephemeral key",
                        ));
                    }
                };
                parts.headers.insert(EPHEMERAL_SIGNATURE_HEADER, signature);
            }

            if let Some(quorum_key) = quorum_key {
                let signature = match signature_header_value(&quorum_key, &signing_payload) {
                    Some(signature) => signature,
                    None => {
                        return Ok(internal_error_response(
                            "failed to sign response with quorum key",
                        ));
                    }
                };
                parts.headers.insert(QUORUM_SIGNATURE_HEADER, signature);
            }

            Ok(Response::from_parts(parts, Body::from(body_bytes)))
        })
    }
}
