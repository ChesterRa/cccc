use cccc_contracts::GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION;
use cccc_core::group_bridge_identity::GroupBridgeIdentity;
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::group_bridge_store::{BridgeStore, items, items_mut};
use crate::AppState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SessionProtocol {
    V1,
    V2,
}

pub(super) fn signed_v2_challenge(state: &AppState) -> Option<Value> {
    let mut challenge = json!({
        "type":"challenge",
        "protocol":"/cccc/group_bridge/session-ws/2.0.0",
        "nonce":uuid::Uuid::new_v4().simple().to_string(),
        "issued_at":cccc_contracts::utc_now(),
        "expires_at":(Utc::now()+Duration::seconds(30)).to_rfc3339(),
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    });
    GroupBridgeIdentity::load_or_create(&state.home)
        .and_then(|identity| identity.sign_session_challenge_v2(&mut challenge))
        .ok()?;
    Some(challenge)
}

pub(super) fn signed_v2_ready(state: &AppState, hello: &Value, challenge: &Value) -> Option<Value> {
    let mut ready = json!({
        "ok":true,"type":"ready",
        "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    });
    GroupBridgeIdentity::load_or_create(&state.home)
        .and_then(|identity| identity.sign_session_ready_v2(&mut ready, hello, challenge))
        .ok()?;
    Some(ready)
}

pub(super) fn authorize_signed_hello(
    state: &AppState,
    hello: &Value,
    protocol: SessionProtocol,
    challenge: Option<&Value>,
) -> Option<Value> {
    let target_group_id = hello["target_group_id"].as_str()?.trim();
    let src_group_id = hello["src_group_id"].as_str()?.trim();
    let remote_peer_id = hello["remote_peer_id"].as_str()?.trim();
    let bridge = BridgeStore::new(&state.home).load().ok()?;
    let trust = items(&bridge, "trusts").iter().find(|item| {
        item["status"] == "active"
            && item["group_id"] == target_group_id
            && item["remote_group_id"] == src_group_id
            && item["remote_peer_id"] == remote_peer_id
    })?;
    if protocol == SessionProtocol::V1 && trust["min_session_protocol"].as_u64().unwrap_or(1) >= 2 {
        return None;
    }
    let authenticated_peer = match protocol {
        SessionProtocol::V2 => {
            cccc_core::group_bridge_identity::authenticated_session_v2_peer_id(hello, challenge?)
        }
        SessionProtocol::V1 if hello["fresh_signature"].is_string() => {
            let peer = cccc_core::group_bridge_identity::authenticated_session_peer_id(hello)?;
            if !issued_at_is_fresh(&hello["issued_at"])
                || !consume_legacy_nonce(&peer, hello["nonce"].as_str()?.trim())
            {
                return None;
            }
            Some(peer)
        }
        SessionProtocol::V1 => {
            cccc_core::group_bridge_identity::authenticated_legacy_session_peer_id(hello)
        }
    }?;
    if authenticated_peer != remote_peer_id {
        return None;
    }
    let registration = items(&bridge, "registrations")
        .iter()
        .find(|registration| {
            registration["status"] == "active"
                && registration["registration_id"] == trust["registration_id"]
                && registration["group_id"] == target_group_id
                && registration["remote_group_id"] == src_group_id
                && registration["remote_peer_id"] == remote_peer_id
        })
        .cloned()
        .unwrap_or_else(|| trust.clone());
    Some(registration)
}

pub(super) fn pin_v2(state: &AppState, registration: &Value) -> Option<()> {
    BridgeStore::new(&state.home)
        .update(|bridge| {
            let trust = items_mut(bridge, "trusts")
                .iter_mut()
                .find(|trust| {
                    trust["status"] == "active"
                        && [
                            "registration_id",
                            "group_id",
                            "remote_group_id",
                            "remote_peer_id",
                        ]
                        .into_iter()
                        .all(|field| trust[field] == registration[field])
                })
                .ok_or_else(|| std::io::Error::other("active Group Bridge trust not found"))?;
            trust["min_session_protocol"] = json!(2);
            trust["updated_at"] = json!(cccc_contracts::utc_now());
            Ok(())
        })
        .ok()
}

fn issued_at_is_fresh(value: &Value) -> bool {
    let Some(issued_at) = value
        .as_str()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
    else {
        return false;
    };
    let delta = Utc::now().signed_duration_since(issued_at);
    delta >= Duration::seconds(-30) && delta <= Duration::seconds(60)
}

fn consume_legacy_nonce(peer_id: &str, nonce: &str) -> bool {
    const MAX_NONCES: usize = 4_096;
    static NONCES: OnceLock<Mutex<HashMap<String, DateTime<Utc>>>> = OnceLock::new();
    if !(16..=128).contains(&nonce.len()) {
        return false;
    }
    let now = Utc::now();
    let Ok(mut nonces) = NONCES.get_or_init(|| Mutex::new(HashMap::new())).lock() else {
        return false;
    };
    nonces.retain(|_, seen_at| now.signed_duration_since(*seen_at) <= Duration::minutes(2));
    let key = format!("{peer_id}:{nonce}");
    if nonces.contains_key(&key) {
        return false;
    }
    if nonces.len() >= MAX_NONCES
        && let Some(oldest) = nonces
            .iter()
            .min_by_key(|(_, seen_at)| **seen_at)
            .map(|(key, _)| key.clone())
    {
        nonces.remove(&oldest);
    }
    nonces.insert(key, now);
    true
}
