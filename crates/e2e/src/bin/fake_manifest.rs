//! Write a fake manifest envelope to a file for local development, the same
//! way QOS writes the real one at boot.
//!
//! DO NOT USE IN PRODUCTION: the generated manifest carries made-up
//! measurements and no approvals.
#![allow(clippy::expect_used)]

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: fake_manifest <path>");
    let envelope = tvc_utils::fake_manifest_envelope();
    std::fs::write(
        &path,
        envelope
            .to_storage_vec()
            .expect("failed to serialize manifest envelope"),
    )
    .expect("failed to write manifest envelope");
}
