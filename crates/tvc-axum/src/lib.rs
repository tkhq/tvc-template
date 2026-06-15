//! Axum adapters for TVC services.
//!
//! This crate contains reusable response adapters that are independent from a
//! specific application crate:
//!
//! - [`QosJson`] serializes response values with `qos_json` canonical JSON.
//! - [`ResponseSigningLayer`] signs every response body with the enclave's
//!   ephemeral `qos_p256` key and attaches hex-encoded signature metadata.

mod qos_json_response;
pub use qos_json_response::QosJson;

mod signing;
pub use signing::{
    PUBLIC_KEY_HEADER, ResponseSigningLayer, ResponseSigningService, SIGNATURE_HEADER,
};
