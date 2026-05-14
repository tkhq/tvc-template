//! Generic HTTP request metrics for Axum services.
//!
//! Provides a Tower middleware layer that automatically records request
//! duration, method, path, and status code as Prometheus histograms,
//! plus a handler for serving the metrics endpoint.

mod layer;
pub use layer::{
    MetricsCollector, MetricsLayer, MetricsLayerBuilder, MetricsService, ResponseFuture,
};

mod handler;
pub use handler::handler;

/// Re-export prometheus for callers who need custom metrics.
pub use prometheus;
