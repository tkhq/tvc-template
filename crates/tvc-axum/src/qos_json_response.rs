//! Axum response adapter for QOS canonical JSON.

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

const APPLICATION_JSON: &str = "application/json";

/// Response adapter that serializes its inner value with `qos_json`.
///
/// `qos_json` produces canonical JSON bytes: object keys are sorted, null
/// object fields are omitted, and integer JSON numbers are normalized to
/// strings. This adapter preserves Axum's normal `application/json` content
/// type while using that canonical serialization for the response body.
pub struct QosJson<T>(
    /// The value serialized as QOS canonical JSON.
    pub T,
);

impl<T> IntoResponse for QosJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        match qos_json::to_vec(&self.0) {
            Ok(body) => {
                let mut response = Response::new(Body::from(body));
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static(APPLICATION_JSON),
                );
                response
            }
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                format!("failed to serialize QOS JSON response: {error}"),
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use serde::Serialize;

    #[derive(Serialize)]
    struct ResponseBody {
        z_value: String,
        count: u64,
        a_value: String,
    }

    #[tokio::test]
    async fn qos_json_response_uses_canonical_json_and_content_type() {
        let response = QosJson(ResponseBody {
            z_value: "last".to_string(),
            count: 42,
            a_value: "first".to_string(),
        })
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            APPLICATION_JSON
        );

        let body = response
            .into_body()
            .collect()
            .await
            .expect("failed to read body")
            .to_bytes();
        assert_eq!(
            body,
            Body::from(r#"{"a_value":"first","count":"42","z_value":"last"}"#)
                .collect()
                .await
                .expect("failed to read expected body")
                .to_bytes()
        );
    }
}
