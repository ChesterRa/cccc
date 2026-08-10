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
                if active.as_ref().is_some_and(|lease| {
                    lease["owner_id"] != owner_id || lease["group_id"] != group_id
                }) {
                    return Ok(Err(redact_lease(active.clone().unwrap_or(Value::Null))));
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
                if !matches_lease(active.as_ref(), group_id, owner_id, lease_id) {
                    lost = true;
                } else if let Some(lease) = active.as_mut() {
                    let expires_at_ms = now_ms + ttl_seconds * 1000;
                    lease["updated_at"] = json!(utc_now());
                    lease["expires_at"] = json!(iso_from_millis(expires_at_ms));
                    lease["expires_at_ms"] = json!(expires_at_ms);
                }
            }
            "release" => {
                if matches_lease(active.as_ref(), group_id, owner_id, lease_id) {
                    active = None;
                    released = true;
                } else {
                    lost = true;
                }
            }
            _ => return Err(std::io::Error::other("unsupported lease action")),
        }

        *stored = active.clone().unwrap_or(Value::Null);
        let response_lease = if action == "status" {
            active.clone().map(redact_lease)
        } else {
            active.clone()
        };
        let response_lease_id = if acquired || action == "heartbeat" && !lost {
            active
                .as_ref()
                .and_then(|lease| lease["lease_id"].as_str())
                .unwrap_or("")
        } else {
            ""
        };
        Ok(Ok(json!({
            "group_id": group_id,
            "action": action,
            "acquired": acquired,
            "released": released,
            "lost": lost,
            "lease_id": response_lease_id,
            "lease": response_lease,
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
    redact_lease(current_private(home))
}

fn current_private(home: &HomeLayout) -> Value {
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

pub(super) fn validate(
    home: &HomeLayout,
    group_id: &str,
    owner_id: &str,
    lease_id: &str,
) -> Result<(), ApiError> {
    let active = current_private(home);
    if active["group_id"].as_str() == Some(group_id)
        && active["owner_id"].as_str() == Some(owner_id)
        && active["lease_id"].as_str() == Some(lease_id)
    {
        return Ok(());
    }
    Err(ApiError::conflict(
        "assistant_voice_recording_lease_lost",
        "voice secretary recording lease is missing, expired, or owned by another client",
        json!({"active_lease":redact_lease(active)}),
    ))
}

/// Keep an authenticated recording lease alive from the active audio stream.
///
/// Python keeps the transcription WebSocket alive independently of the HTTP
/// heartbeat. Rust still validates the lease at connection time, then renews it
/// from received audio so a transient HTTP failure cannot terminate a healthy
/// recording session.
pub(super) fn renew(
    home: &HomeLayout,
    group_id: &str,
    owner_id: &str,
    lease_id: &str,
) -> Result<bool, ApiError> {
    let result = update(
        home,
        group_id,
        &json!({
            "action": "heartbeat",
            "owner_id": owner_id,
            "lease_id": lease_id,
            "ttl_seconds": DEFAULT_TTL_SECONDS,
        }),
    )?;
    Ok(!result["lost"].as_bool().unwrap_or(true))
}

/// Release the lease owned by a completed WebSocket connection.
///
/// Ownership is checked atomically by `update`, so a stale connection cannot
/// release a lease that has already been replaced by another recorder.
pub(super) fn release(
    home: &HomeLayout,
    group_id: &str,
    owner_id: &str,
    lease_id: &str,
) -> Result<bool, ApiError> {
    let result = update(
        home,
        group_id,
        &json!({
            "action": "release",
            "owner_id": owner_id,
            "lease_id": lease_id,
        }),
    )?;
    Ok(result["released"].as_bool().unwrap_or(false))
}

fn active_lease(stored: &Value, now_ms: i64) -> Option<Value> {
    stored.is_object().then(|| stored.clone()).filter(|lease| {
        lease["expires_at_ms"]
            .as_i64()
            .is_some_and(|expiry| expiry > now_ms)
    })
}

fn matches_lease(active: Option<&Value>, group_id: &str, owner_id: &str, lease_id: &str) -> bool {
    active.is_some_and(|lease| {
        lease["group_id"].as_str() == Some(group_id)
            && lease["owner_id"].as_str() == Some(owner_id)
            && lease["lease_id"].as_str() == Some(lease_id)
    })
}

fn redact_lease(mut lease: Value) -> Value {
    if let Some(value) = lease.as_object_mut() {
        value.remove("lease_id");
    }
    lease
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
        assert!(current(&home).get("lease_id").is_none());
        assert!(
            validate(
                &home,
                &first.group_id,
                "tab-1",
                acquired["lease_id"].as_str().expect("lease id")
            )
            .is_ok()
        );
        assert!(validate(&home, &first.group_id, "tab-2", "wrong").is_err());
        let redacted = redact_lease(current_private(&home));
        assert!(redacted.is_object());
        assert!(redacted.get("lease_id").is_none());
        assert!(
            update(
                &home,
                &second.group_id,
                &json!({"action":"release","owner_id":"tab-1","lease_id":acquired["lease_id"]}),
            )
            .expect("cross-group release")["lost"]
                .as_bool()
                .unwrap_or(false)
        );
        assert!(
            validate(
                &home,
                &first.group_id,
                "tab-1",
                acquired["lease_id"].as_str().expect("lease")
            )
            .is_ok()
        );
    }

    #[test]
    fn active_audio_renewal_extends_the_recording_lease() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("recording", "").expect("group");
        let acquired = update(
            &home,
            &group.group_id,
            &json!({"action":"acquire","owner_id":"tab-1","ttl_seconds":5}),
        )
        .expect("acquire");
        let lease_id = acquired["lease_id"].as_str().expect("lease id");
        let original_expiry = current_private(&home)["expires_at_ms"]
            .as_i64()
            .expect("original expiry");

        assert!(renew(&home, &group.group_id, "tab-1", lease_id).expect("renew"));

        let renewed_expiry = current_private(&home)["expires_at_ms"]
            .as_i64()
            .expect("renewed expiry");
        assert!(renewed_expiry > original_expiry);
        assert!(!renew(&home, &group.group_id, "tab-2", lease_id).expect("lost"));

        assert!(release(&home, &group.group_id, "tab-1", lease_id).expect("release"));
        assert!(current_private(&home).is_null());
        assert!(!release(&home, &group.group_id, "tab-1", lease_id).expect("idempotent release"));
    }
}
