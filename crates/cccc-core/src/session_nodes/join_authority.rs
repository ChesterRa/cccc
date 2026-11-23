//! Retain non-secret revocation metadata for mixed-version state readers.
use super::store;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Revocation {
    generation: String,
    expires_at: DateTime<Utc>,
}

pub(super) fn revoked(state: &mut store::State, node: &str) {
    let now = Utc::now();
    state.revocations.retain(|_, value| value.expires_at > now);
    state.revocations.insert(
        node.into(),
        Revocation {
            generation: uuid::Uuid::new_v4().to_string(),
            expires_at: now + Duration::seconds(600),
        },
    );
}
