use cccc_contracts::utc_now;
use cccc_core::{GroupStore, HomeLayout, integration_state};
use serde_json::{Value, json};

use crate::api::ApiError;

const STORE_KEY: &str = "voice_secretary_recording_lease";
const DEFAULT_TTL_SECONDS: i64 = 30;
const MIN_TTL_SECONDS: i64 = 5;
const MAX_TTL_SECONDS: i64 = 120;

pub(super) fn update(home: &HomeLayout, group_id: &str, body: &Value) -> Result<Value, ApiError> {
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    let action = body["action"].as_str().unwrap_or("status");
    let owner_id = body["owner_id"].as_str().unwrap_or("").trim();
    let lease_id = body["lease_id"].as_str().unwrap_or("").trim();
    if action != "status" && owner_id.is_empty() {
        return Err(ApiError::bad("owner_id is required"));
    }
    if matches!(action, "heartbeat" | "release") && lease_id.is_empty() {
        return Err(ApiError::bad("lease_id is required"));
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let ttl_seconds = body["ttl_seconds"]
        .as_i64()
        .unwrap_or(DEFAULT_TTL_SECONDS)
        .clamp(MIN_TTL_SECONDS, MAX_TTL_SECONDS);
    let result = integration_state::global_update(home, STORE_KEY, |stored| {
        let mut active = active_lease(stored, now_ms);
        let mut acquired = false;
        let mut released = false;
        let mut lost = false;

        match action {
            "status" => {}
            "acquire" => {
                if active
                    .as_ref()
                    .is_some_and(|lease| lease["owner_id"] != owner_id)
                {
                    return Ok(Err(active.clone().unwrap_or(Value::Null)));
                }
                let expires_at_ms = now_ms + ttl_seconds * 1000;
                let existing_id = active
                    .as_ref()
                    .and_then(|lease| lease["lease_id"].as_str())
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("vrl_{}", uuid::Uuid::new_v4().simple()));
                let created_at = active
                    .as_ref()
                    .and_then(|lease| lease["created_at"].as_str())
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(utc_now);
                active = Some(json!({
                    "lease_id": existing_id,
                    "owner_id": owner_id,
                    "group_id": group_id,
                    "group_title": group.title,
                    "capture_mode": body["capture_mode"],
                    "recognition_backend": body["recognition_backend"],
                    "by": body["by"].as_str().unwrap_or("user"),
                    "created_at": created_at,
                    "updated_at": utc_now(),
                    "expires_at": iso_from_millis(expires_at_ms),
                    "expires_at_ms": expires_at_ms,
                }));
                acquired = true;
            }
            "heartbeat" => {
                if !matches_lease(active.as_ref(), owner_id, lease_id) {
                    lost = true;
                } else if let Some(lease) = active.as_mut() {
                    let expires_at_ms = now_ms + ttl_seconds * 1000;
                    lease["updated_at"] = json!(utc_now());
                    lease["expires_at"] = json!(iso_from_millis(expires_at_ms));
                    lease["expires_at_ms"] = json!(expires_at_ms);
                }
            }
            "release" => {
                if matches_lease(active.as_ref(), owner_id, lease_id) {
                    active = None;
                    released = true;
                } else {
                    lost = true;
                }
            }
            _ => return Err(std::io::Error::other("unsupported lease action")),
        }

        *stored = active.clone().unwrap_or(Value::Null);
        Ok(Ok(json!({
            "group_id": group_id,
            "action": action,
            "acquired": acquired,
            "released": released,
            "lost": lost,
            "lease_id": active.as_ref().map_or("", |lease| lease["lease_id"].as_str().unwrap_or("")),
            "lease": active,
        })))
    })
    .map_err(|error| ApiError::bad(error.to_string()))?;

    result.map_err(|active| {
        ApiError::conflict(
            "assistant_voice_recording_busy",
            "voice secretary recording is already active",
            json!({"active_lease": active}),
        )
    })
}

pub(super) fn current(home: &HomeLayout) -> Value {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let stored = integration_state::global_get(home, STORE_KEY).unwrap_or(Value::Null);
    if let Some(active) = active_lease(&stored, now_ms) {
        return active;
    }
    if stored.is_object() {
        let _ = integration_state::global_update(home, STORE_KEY, |value| {
            *value = Value::Null;
            Ok(())
        });
    }
    Value::Null
}

fn active_lease(stored: &Value, now_ms: i64) -> Option<Value> {
    stored.is_object().then(|| stored.clone()).filter(|lease| {
        lease["expires_at_ms"]
            .as_i64()
            .is_some_and(|expiry| expiry > now_ms)
    })
}

fn matches_lease(active: Option<&Value>, owner_id: &str, lease_id: &str) -> bool {
    active.is_some_and(|lease| {
        lease["owner_id"].as_str() == Some(owner_id) && lease["lease_id"].as_str() == Some(lease_id)
    })
}

fn iso_from_millis(value: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(value)
        .map_or_else(utc_now, |value| value.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_is_global_and_requires_matching_owner_and_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let first = store.create("first", "").expect("first");
        let second = store.create("second", "").expect("second");
        let acquired = update(
            &home,
            &first.group_id,
            &json!({"action":"acquire","owner_id":"tab-1","ttl_seconds":30}),
        )
        .expect("acquire");
        assert!(
            update(
                &home,
                &second.group_id,
                &json!({"action":"acquire","owner_id":"tab-2","ttl_seconds":30}),
            )
            .is_err()
        );
        assert!(
            update(
                &home,
                &first.group_id,
                &json!({"action":"release","owner_id":"tab-2","lease_id":acquired["lease_id"]}),
            )
            .expect("mismatched release")["lost"]
                .as_bool()
                .unwrap_or(false)
        );
        assert!(current(&home).is_object());
    }
}
