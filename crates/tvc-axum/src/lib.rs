//! Axum adapters for Turnkey Verifiable Cloud applications.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes, HttpBody};
use axum::http::{HeaderValue, Request, Response, StatusCode, header};
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use qos_core::handles::EphemeralKeyHandle;
use serde::Serialize;

const EPHEMERAL_PUBLIC_KEY_HEADER: &str = "x-tvc-ephemeral-public-key";
const RESPONSE_SIGNATURE_HEADER: &str = "x-tvc-response-signature";

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

/// Tower layer that signs every response body with the TVC ephemeral P-256 key.
#[derive(Clone)]
pub struct ResponseSigningLayer {
    ephemeral_key_handle: EphemeralKeyHandle<String>,
}

impl ResponseSigningLayer {
    /// Create a response signing layer using the provided ephemeral key handle.
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
#[derive(Clone)]
pub struct ResponseSigningService<S> {
    inner: S,
    ephemeral_key_handle: EphemeralKeyHandle<String>,
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
        let ephemeral_key_handle = self.ephemeral_key_handle.clone();

        Box::pin(async move {
            let response = future.await?;
            let (mut parts, body) = response.into_parts();
            let body_bytes = match body.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => return Ok(internal_error_response("failed to read response body")),
            };

            let ephemeral_key = match ephemeral_key_handle.get_ephemeral_key() {
                Ok(key) => key,
                Err(_) => return Ok(internal_error_response("failed to load ephemeral key")),
            };
            let signature = match ephemeral_key.sign(&body_bytes) {
                Ok(signature) => signature,
                Err(_) => return Ok(internal_error_response("failed to sign response")),
            };
            let public_key = qos_hex::encode(&ephemeral_key.public_key().to_bytes());
            let signature = qos_hex::encode(&signature);

            let public_key = match HeaderValue::from_str(&public_key) {
                Ok(public_key) => public_key,
                Err(_) => {
                    return Ok(internal_error_response(
                        "failed to encode public key header",
                    ));
                }
            };
            let signature = match HeaderValue::from_str(&signature) {
                Ok(signature) => signature,
                Err(_) => return Ok(internal_error_response("failed to encode signature header")),
            };

            parts
                .headers
                .insert(EPHEMERAL_PUBLIC_KEY_HEADER, public_key);
            parts.headers.insert(RESPONSE_SIGNATURE_HEADER, signature);

            Ok(Response::from_parts(parts, Body::from(body_bytes)))
        })
    }
}
