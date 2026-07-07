//! Downloader server to test both ingress and egress pivot functionality

use std::{
    net::SocketAddr,
    time::{Duration, SystemTime},
};

use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use ureq::Resolver;

struct FixedResolver {
    ip_addr: SocketAddr,
}

// download request
#[derive(Debug, Deserialize)]
pub(crate) struct DlRequest {
    ip_override: Option<String>,
}

#[derive(Debug, Serialize)]
struct DlResponse {
    status: u16,
    size: usize,
    duration: Duration,
    sha256sum: String,
    error: Option<String>,
}

impl DlResponse {
    pub fn error(code: StatusCode, msg: String) -> Json<Self> {
        Json(Self {
            status: code.as_u16(),
            size: 0,
            duration: Duration::ZERO,
            sha256sum: String::new(),
            error: Some(msg),
        })
    }
}

impl Resolver for FixedResolver {
    fn resolve(&self, _netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        Ok(vec![self.ip_addr])
    }
}

pub(crate) async fn download(
    Query(DlRequest { ip_override }): Query<DlRequest>,
) -> impl IntoResponse {
    match tokio::task::spawn_blocking(move || {
        let url = "https://bitcoin.org/bitcoin.pdf";
        let client_builder = if let Some(ip) = ip_override {
            let ip_addr = match ip.parse() {
                Ok(ip) => ip,
                Err(err) => {
                    let code = StatusCode::BAD_REQUEST;
                    let err = format!("{err}");
                    return (code, DlResponse::error(code, err));
                }
            };

            tracing::info!("using override ip of {ip} for base url {url}");

            ureq::builder().resolver(FixedResolver { ip_addr })
        } else {
            ureq::builder()
        };

        let client = client_builder.timeout(Duration::from_mins(1)).build();

        tracing::info!("starting download of {url}");
        let request = client.get(url.as_ref());

        let start = SystemTime::now();
        let dl = match request.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let code = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                return (code, DlResponse::error(code, r.status_text().to_string()));
            }
            Err(ureq::Error::Transport(e)) => {
                let code = StatusCode::INTERNAL_SERVER_ERROR;
                return (code, DlResponse::error(code, e.to_string()));
            }
        };

        let status = dl.status();
        let bytes: Vec<u8> = match dl.into_string() {
            Ok(body) => body,
            Err(err) => {
                let code = StatusCode::BAD_REQUEST;
                return (code, DlResponse::error(code, err.to_string()));
            }
        }
        .bytes()
        .collect();

        let size = bytes.len();
        let sha256sum = sha2::Sha256::digest(bytes);
        let duration = SystemTime::now()
            .duration_since(start)
            .unwrap_or(Duration::ZERO);

        tracing::info!(
            "download complete\n\tstatus: {}\n\tsize: {}\n\tduration: {:?}\n\tsha256sum:{:x}",
            status,
            size,
            duration,
            sha256sum,
        );

        (
            StatusCode::OK,
            Json(DlResponse {
                status,
                size,
                duration,
                sha256sum: format!("{sha256sum:x}"),
                error: None,
            }),
        )
    })
    .await
    {
        Ok(val) => val,
        Err(err) => {
            let code = StatusCode::INTERNAL_SERVER_ERROR;
            (code, DlResponse::error(code, err.to_string()))
        }
    }
}
