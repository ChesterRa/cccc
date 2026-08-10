use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use sha2::Digest;
use std::io;
use uuid::Uuid;

pub struct BridgeStore<'a> {
    home: &'a HomeLayout,
}

impl<'a> BridgeStore<'a> {
    pub fn new(home: &'a HomeLayout) -> Self {
        Self { home }
    }

    pub fn load(&self) -> io::Result<Value> {
        let mut value = cccc_core::group_bridge_legacy::load(self.home)?;
        normalize(&mut value);
        Ok(value)
    }

    pub fn update<T>(
        &self,
        change: impl FnOnce(&mut Map<String, Value>) -> io::Result<T>,
    ) -> io::Result<T> {
        cccc_core::group_bridge_legacy::update(self.home, |value| {
            ensure_sections(value);
            change(value)
        })
    }

    /// Persists the legacy `active` → `approved` outbound repair exactly once when
    /// needed. Reads raw storage (before `normalize` rewrites memory) to detect
    /// whether a repair is pending, and only writes when one is — so a steady-state
    /// store incurs no extra writes. The matching logic is shared with `normalize`.
    pub fn repair_legacy_active_outbounds(&self) -> io::Result<()> {
        let mut raw = cccc_core::group_bridge_legacy::load(self.home)?;
        let needs_repair = {
            if !raw.is_object() {
                raw = json!({});
            }
            let state = raw.as_object_mut().expect("bridge store initialized");
            ensure_sections(state);
            normalize_legacy_active_outbounds(state)
        };
        if !needs_repair {
            return Ok(());
        }
        self.update(|state| {
            normalize_legacy_active_outbounds(state);
            Ok(())
        })?;
        Ok(())
    }

    pub fn identity(&self) -> io::Result<Value> {
        let signing =
            cccc_core::group_bridge_identity::GroupBridgeIdentity::load_or_create(self.home)?;
        let digest = format!("{:x}", sha2::Sha256::digest(signing.peer_id.as_bytes()));
        let node_id = format!("node_{}", &digest[..24]);
        self.update(|state| {
            let identity = json!({
                "node_id":node_id,
                "peer_id":signing.peer_id
            });
            state.insert("identity".into(), identity.clone());
            Ok(identity.clone())
        })
    }
}

pub fn items<'a>(state: &'a Value, key: &str) -> &'a [Value] {
    state
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub fn items_mut<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    let value = state.entry(key).or_insert_with(|| json!([]));
    if !value.is_array() {
        *value = json!([]);
    }
    value.as_array_mut().expect("bridge section initialized")
}

pub fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_owned()
}

fn normalize(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
    let state = value.as_object_mut().expect("bridge store initialized");
    ensure_sections(state);
    normalize_legacy_active_outbounds(state);
}

fn ensure_sections(state: &mut Map<String, Value>) {
    for key in [
        "invites",
        "requests",
        "trusts",
        "registrations",
        "outbounds",
        "deliveries",
    ] {
        state.entry(key).or_insert_with(|| json!([]));
    }
}

/// Back-compat for an old `sync_outbound` bug: a successfully paired outbound was
/// written with `status = "active"` instead of the pairing-flow terminal state
/// `"approved"`. Because the frontend only re-syncs `submitted`/`pending`
/// outbounds, those stale `active` records never reach the `approved` write path
/// again and stay pinned in the "sent requests" list forever.
///
/// This folds them back to `approved` — but only when a matching `active` trust
/// proves the pairing actually completed and routing is live. An `active`
/// outbound with no matching trust is left untouched: it may be a genuine
/// failure or orphan, and silently hiding it would destroy the audit trail.
///
/// Identity match mirrors the routing lookup in `group_bridge_session`
/// (group_id + remote_group_id + remote_peer_id, trust `active`). Returns true
/// when at least one outbound was normalized so callers can persist once.
fn normalize_legacy_active_outbounds(state: &mut Map<String, Value>) -> bool {
    let trusts = state
        .get("trusts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_active_trust = |outbound: &Value| {
        let local_group_id = outbound.get("local_group_id");
        let issuer_group_id = outbound.get("issuer_group_id");
        let issuer_peer_id = outbound.get("issuer_peer_id");
        trusts.iter().any(|trust| {
            trust.get("status") == Some(&json!("active"))
                && trust.get("group_id") == local_group_id
                && trust.get("remote_group_id") == issuer_group_id
                && trust.get("remote_peer_id") == issuer_peer_id
        })
    };
    let outbounds = match state.get_mut("outbounds").and_then(Value::as_array_mut) {
        Some(items) => items,
        None => return false,
    };
    let mut changed = false;
    for outbound in outbounds.iter_mut() {
        if outbound.get("status") == Some(&json!("active")) && has_active_trust(outbound) {
            outbound["status"] = json!("approved");
            outbound["updated_at"] = json!(utc_now());
            changed = true;
        }
    }
    changed
}
