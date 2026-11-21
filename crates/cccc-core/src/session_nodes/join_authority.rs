//! Non-secret, per-device revocation generations outlive pending approvals.
use super::store;
use crate::HomeLayout;
use cccc_contracts::session_node_join::JOIN_TTL_SECONDS;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Serialize, Deserialize)]
pub(super) struct Revocation {
    generation: String,
    expires_at: DateTime<Utc>,
}

pub(super) fn current<'a>(state: &'a store::State, node: &str, now: DateTime<Utc>) -> &'a str {
    state
        .revocations
        .get(node)
        .filter(|value| value.expires_at > now)
        .map(|value| value.generation.as_str())
        .unwrap_or("")
}

pub fn capture_join_generation(
    home: &HomeLayout,
    node: &str,
    expires_at: &str,
) -> io::Result<String> {
    let deadline = expires_at
        .parse::<DateTime<Utc>>()
        .map_err(|_| store::denied())?;
    let now = Utc::now();
    if deadline <= now || deadline > now + Duration::seconds(JOIN_TTL_SECONDS as i64) {
        return Err(store::denied());
    }
    store::update(home, |state| {
        state.revocations.retain(|_, value| value.expires_at > now);
        if let Some(value) = state.revocations.get_mut(node) {
            // A request near the old retention deadline must keep its baseline
            // through its own TTL, even if the peer history is later evicted.
            value.expires_at = value.expires_at.max(deadline);
            Ok(value.generation.clone())
        } else {
            if state.revocations.len() >= 256 {
                return Err(io::Error::other("Device enrollment state is full"));
            }
            let generation = uuid::Uuid::new_v4().to_string();
            state.revocations.insert(
                node.into(),
                Revocation {
                    generation: generation.clone(),
                    expires_at: deadline,
                },
            );
            Ok(generation)
        }
    })
}

pub fn join_generation_matches(
    home: &HomeLayout,
    node: &str,
    generation: &str,
) -> io::Result<bool> {
    Ok(!generation.is_empty() && current(&store::read(home)?, node, Utc::now()) == generation)
}

pub(super) fn revoked(state: &mut store::State, node: &str) {
    let now = Utc::now();
    state.revocations.retain(|_, value| value.expires_at > now);
    state.revocations.insert(
        node.into(),
        Revocation {
            generation: uuid::Uuid::new_v4().to_string(),
            expires_at: now + Duration::seconds(JOIN_TTL_SECONDS as i64),
        },
    );
}
