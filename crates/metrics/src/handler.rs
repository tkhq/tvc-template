//! Axum handler for serving Prometheus metrics.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{self, MethodRouter};

use crate::MetricsCollector;

/// Prometheus content type for text format.
const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Returns an Axum [`MethodRouter`] that serves Prometheus metrics.
///
/// Mount this on any path:
///
/// ```rust,no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use metrics::{MetricsLayer, handler};
/// use axum::Router;
///
/// let layer = MetricsLayer::builder().build()?;
/// let app = Router::new()
///     .route("/metrics", handler(layer.collector()));
/// # Ok(())
/// # }
/// ```
pub fn handler(collector: MetricsCollector) -> MethodRouter {
    routing::get(move || {
        let collector = collector.clone();
        async move { serve_metrics(&collector) }
    })
}

fn serve_metrics(collector: &MetricsCollector) -> Response {
    match collector.encode() {
        Ok(body) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
            body,
        )
            .into_response(),
        Err(e) => {
            let msg = format!("metrics encoding failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
        }
    }
}
