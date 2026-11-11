//! The executable registers its compiled source identity before starting services.
//! Library embeddings without one omit this optional diagnostic; this module
//! never reads the checkout or invents an identity for the embedding program.
//! This is not a binary checksum or a protocol compatibility requirement.

use std::sync::OnceLock;

static SOURCE_ID: OnceLock<&'static str> = OnceLock::new();

/// Call at the executable entry point with its compile-time source fingerprint.
pub fn initialize(source_id: &'static str) {
    assert!(
        source_id.len() == 64 && source_id.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "application source identity must be a SHA-256 fingerprint"
    );
    if SOURCE_ID.set(source_id).is_err() {
        assert_eq!(SOURCE_ID.get().copied(), Some(source_id));
    }
}

pub fn source_id() -> Option<&'static str> {
    SOURCE_ID.get().copied()
}

pub fn current() -> serde_json::Value {
    match source_id() {
        Some(source_id) => serde_json::json!({"source_id": source_id}),
        None => serde_json::json!({}),
    }
}
