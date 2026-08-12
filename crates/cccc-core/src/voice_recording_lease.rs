use chrono::Utc;
use serde_json::{Map, Value, json};
use std::io;

use crate::{HomeLayout, fs};

const DEFAULT_TTL_SECONDS: i64 = 30;
const MIN_TTL_SECONDS: i64 = 5;
const MAX_TTL_SECONDS: i64 = 120;
const STATE_FILE: &str = "state/voice_secretary_recording_lease.json";

#[derive(Debug, Clone)]
pub struct LeaseError {
    pub code: &'static str,
    pub message: String,
    pub details: Map<String, Value>,
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LeaseError {}

impl LeaseError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Map::new(),
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = details.as_object().cloned().unwrap_or_default();
        self
    }
}

impl From<io::Error> for LeaseError {
    fn from(error: io::Error) -> Self {
        Self::new("assistant_voice_recording_lease_failed", error.to_string())
    }
}

pub fn update(
    home: &HomeLayout,
    group_id: &str,
    group_title: &str,
    body: &Value,
) -> Result<Value, LeaseError> {
    let action = body["action"].as_str().unwrap_or("status").trim();
    let owner_id = body["owner_id"].as_str().unwrap_or("").trim();
    let lease_id = body["lease_id"].as_str().unwrap_or("").trim();
    if !matches!(action, "acquire" | "heartbeat" | "release" | "status") {
        return Err(LeaseError::new(
            "invalid_voice_recording_lease_action",
            "action must be acquire|heartbeat|release|status",
        ));
    }
    if action != "status" && owner_id.is_empty() {
        return Err(LeaseError::new("missing_owner_id", "missing owner_id"));
    }
    if matches!(action, "heartbeat" | "release") && lease_id.is_empty() {
        return Err(LeaseError::new("missing_lease_id", "missing lease_id"));
    }

    let now_ms = Utc::now().timestamp_millis();
    let ttl_seconds = body["ttl_seconds"]
        .as_i64()
        .unwrap_or(DEFAULT_TTL_SECONDS)
        .clamp(MIN_TTL_SECONDS, MAX_TTL_SECONDS);
    with_lock(home, || {
        let mut active = active_lease_locked(home, now_ms)?;
        let mut acquired = false;
        let mut released = false;
        let mut lost = false;

        match action {
            "status" => {}
            "release" => {
                if matches_lease(active.as_ref(), group_id, owner_id, lease_id) {
                    active = None;
                    released = true;
                    save(home, active.as_ref())?;
                } else {
                    lost = true;
                }
            }
            "acquire" => {
                if active.as_ref().is_some_and(|lease| {
                    lease["owner_id"] != owner_id || lease["group_id"] != group_id
                }) {
                    return Err(LeaseError::new(
                        "assistant_voice_recording_busy",
                        "voice secretary recording is already active",
                    )
                    .with_details(json!({
                        "active_lease": active.as_ref().map(public_lease).unwrap_or_else(||json!({}))
                    })));
                }
                active = Some(next_lease(
                    active.as_ref(),
                    group_id,
                    group_title,
                    owner_id,
                    body,
                    ttl_seconds,
                    now_ms,
                ));
                acquired = true;
                save(home, active.as_ref())?;
            }
            "heartbeat" => {
                if active
                    .as_ref()
                    .is_some_and(|lease| lease["owner_id"] != owner_id)
                {
                    return Err(LeaseError::new(
                        "assistant_voice_recording_busy",
                        "voice secretary recording is already active",
                    )
                    .with_details(json!({
                        "active_lease": active.as_ref().map(public_lease).unwrap_or_else(||json!({}))
                    })));
                }
                if !matches_lease(active.as_ref(), group_id, owner_id, lease_id) {
                    lost = true;
                } else {
                    active = Some(next_lease(
                        active.as_ref(),
                        group_id,
                        group_title,
                        owner_id,
                        body,
                        ttl_seconds,
                        now_ms,
                    ));
                    acquired = true;
                    save(home, active.as_ref())?;
                }
            }
            _ => unreachable!("action validated above"),
        }

        let public = active
            .as_ref()
            .map(public_lease)
            .unwrap_or_else(|| json!({}));
        let response_lease_id = if acquired {
            active
                .as_ref()
                .and_then(|lease| lease["lease_id"].as_str())
                .unwrap_or("")
        } else {
            ""
        };
        let mut result = json!({
            "group_id": group_id,
            "action": action,
            "acquired": acquired,
            "released": released,
            "lost": lost,
            "lease": public,
        });
        if !response_lease_id.is_empty() {
            result["lease_id"] = json!(response_lease_id);
        }
        Ok(result)
    })
}

pub fn current(home: &HomeLayout) -> Result<Value, LeaseError> {
    with_lock(home, || {
        Ok(active_lease_locked(home, Utc::now().timestamp_millis())?
            .as_ref()
            .map(public_lease)
            .unwrap_or_else(|| json!({})))
    })
}

pub fn validate(
    home: &HomeLayout,
    group_id: &str,
    owner_id: &str,
    lease_id: &str,
) -> Result<Value, LeaseError> {
    with_lock(home, || {
        let active = active_lease_locked(home, Utc::now().timestamp_millis())?;
        if let Some(lease) = active.as_ref().filter(|lease| {
            lease["group_id"] == group_id
                && lease["owner_id"] == owner_id
                && lease["lease_id"] == lease_id
        }) {
            return Ok(public_lease(lease));
        }
        Err(LeaseError::new(
            "assistant_voice_recording_lease_lost",
            "voice secretary recording lease is missing, expired, or owned by another client",
        )
        .with_details(json!({
            "active_lease": active.as_ref().map(public_lease).unwrap_or_else(||json!({}))
        })))
    })
}

pub fn renew(
    home: &HomeLayout,
    group_id: &str,
    group_title: &str,
    owner_id: &str,
    lease_id: &str,
) -> Result<bool, LeaseError> {
    let result = update(
        home,
        group_id,
        group_title,
        &json!({
            "action": "heartbeat",
            "owner_id": owner_id,
            "lease_id": lease_id,
            "ttl_seconds": DEFAULT_TTL_SECONDS,
        }),
    )?;
    Ok(!result["lost"].as_bool().unwrap_or(true))
}

/// Release the lease owned by a completed recording connection.
///
/// The group, owner, and private lease ID must all still match, so cleanup from
/// an old connection cannot release a newer recorder's lease.
pub fn release(
    home: &HomeLayout,
    group_id: &str,
    owner_id: &str,
    lease_id: &str,
) -> Result<bool, LeaseError> {
    let result = update(
        home,
        group_id,
        "",
        &json!({
            "action": "release",
            "owner_id": owner_id,
            "lease_id": lease_id,
        }),
    )?;
    Ok(result["released"].as_bool().unwrap_or(false))
}

fn next_lease(
    active: Option<&Value>,
    group_id: &str,
    group_title: &str,
    owner_id: &str,
    body: &Value,
    ttl_seconds: i64,
    now_ms: i64,
) -> Value {
    let generation = active.filter(|_| body["action"] == "heartbeat");
    let created_at = generation
        .and_then(|lease| lease["created_at"].as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(cccc_contracts::utc_now);
    let lease_id = generation
        .and_then(|lease| lease["lease_id"].as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("vrl_{}", uuid::Uuid::new_v4().simple()));
    let expires_at_ms = now_ms + ttl_seconds * 1000;
    let capture_mode = lease_field(active, body, "capture_mode");
    let recognition_backend = lease_field(active, body, "recognition_backend");
    let dispatch_target = lease_field(active, body, "dispatch_target");
    let by = body
        .get("by")
        .and_then(Value::as_str)
        .or_else(|| active.and_then(|lease| lease["by"].as_str()))
        .unwrap_or("user");
    json!({
        "lease_id": lease_id,
        "owner_id": owner_id,
        "group_id": group_id,
        "group_title": group_title,
        "capture_mode": capture_mode,
        "recognition_backend": recognition_backend,
        "dispatch_target": dispatch_target,
        "by": by,
        "created_at": created_at,
        "updated_at": cccc_contracts::utc_now(),
        "expires_at": iso_from_millis(expires_at_ms),
        "expires_at_ms": expires_at_ms,
    })
}

fn lease_field(active: Option<&Value>, body: &Value, key: &str) -> Value {
    body.get(key)
        .cloned()
        .or_else(|| active.and_then(|lease| lease.get(key)).cloned())
        .unwrap_or(Value::Null)
}

fn matches_lease(active: Option<&Value>, group_id: &str, owner_id: &str, lease_id: &str) -> bool {
    active.is_some_and(|lease| {
        lease["group_id"] == group_id
            && lease["owner_id"] == owner_id
            && lease["lease_id"]
                .as_str()
                .map_or(lease_id.is_empty(), |active_id| active_id == lease_id)
    })
}

fn public_lease(lease: &Value) -> Value {
    let mut public = lease.clone();
    if let Some(value) = public.as_object_mut() {
        value.remove("lease_id");
        value.remove("expires_at_ms");
        value.retain(|_, item| !item.is_null() && item.as_str() != Some(""));
    }
    public
}

fn active_lease_locked(home: &HomeLayout, now_ms: i64) -> Result<Option<Value>, LeaseError> {
    let active = load(home);
    if active.as_ref().is_some_and(|lease| {
        lease["expires_at_ms"]
            .as_i64()
            .is_some_and(|expiry| expiry > now_ms)
    }) {
        return Ok(active);
    }
    if active.is_some() {
        save(home, None)?;
    }
    Ok(None)
}

fn load(home: &HomeLayout) -> Option<Value> {
    let payload = fs::read_json::<Value>(&state_path(home)).ok()?;
    (payload["schema"].as_u64() == Some(1))
        .then(|| payload["lease"].clone())
        .filter(Value::is_object)
        .filter(|lease| !lease.as_object().is_none_or(Map::is_empty))
}

fn save(home: &HomeLayout, lease: Option<&Value>) -> Result<(), LeaseError> {
    fs::write_secret_json(
        &state_path(home),
        &json!({
            "schema": 1,
            "kind": "voice_secretary_recording_lease",
            "lease": lease.cloned().unwrap_or_else(|| json!({})),
        }),
    )?;
    Ok(())
}

fn with_lock<T>(
    home: &HomeLayout,
    operation: impl FnOnce() -> Result<T, LeaseError>,
) -> Result<T, LeaseError> {
    let mut result = None;
    fs::with_exclusive_lock(&state_path(home).with_extension("json.lock"), || {
        result = Some(operation());
        Ok(())
    })?;
    result.expect("lease operation always runs while the lock is held")
}

fn state_path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join(STATE_FILE)
}

fn iso_from_millis(value: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(value)
        .map_or_else(cccc_contracts::utc_now, |value| value.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_is_global_redacted_and_token_guarded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let acquired = update(
            &home,
            "g_one",
            "One",
            &json!({
                "action":"acquire",
                "owner_id":"tab-1",
                "ttl_seconds":30,
                "capture_mode":"push_to_talk",
                "recognition_backend":"browser",
                "dispatch_target":{"kind":"composer"},
                "by":"peer1"
            }),
        )
        .expect("acquire");
        let lease_id = acquired["lease_id"].as_str().expect("lease id");
        assert!(acquired["lease"].get("lease_id").is_none());
        assert_eq!(current(&home).expect("current")["owner_id"], "tab-1");
        assert!(
            update(
                &home,
                "g_two",
                "Two",
                &json!({"action":"acquire","owner_id":"tab-2"}),
            )
            .is_err()
        );
        assert!(validate(&home, "g_one", "tab-1", lease_id).is_ok());
        assert!(validate(&home, "g_one", "tab-2", lease_id).is_err());
        assert!(renew(&home, "g_one", "One", "tab-1", lease_id).expect("renew"));
        let renewed = current(&home).expect("renewed");
        assert_eq!(renewed["capture_mode"], "push_to_talk");
        assert_eq!(renewed["recognition_backend"], "browser");
        assert_eq!(renewed["dispatch_target"], json!({"kind":"composer"}));
        assert_eq!(renewed["by"], "peer1");
        assert!(!release(&home, "g_two", "tab-1", lease_id).expect("wrong group"));
        assert_eq!(current(&home).expect("still active")["owner_id"], "tab-1");
        assert!(release(&home, "g_one", "tab-1", lease_id).expect("release"));
        assert!(!release(&home, "g_one", "tab-1", lease_id).expect("idempotent release"));
        assert_eq!(current(&home).expect("empty"), json!({}));
    }

    #[test]
    fn same_owner_reacquire_fences_the_old_connection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let first = update(
            &home,
            "g_one",
            "One",
            &json!({
                "action":"acquire",
                "owner_id":"tab-1",
                "capture_mode":"document",
            }),
        )
        .expect("first acquire");
        let first_id = first["lease_id"]
            .as_str()
            .expect("first lease id")
            .to_owned();

        let second = update(
            &home,
            "g_one",
            "One",
            &json!({"action":"acquire","owner_id":"tab-1"}),
        )
        .expect("replacement acquire");
        let second_id = second["lease_id"]
            .as_str()
            .expect("second lease id")
            .to_owned();
        assert_ne!(second_id, first_id);
        assert_eq!(second["lease"]["capture_mode"], "document");
        assert!(!release(&home, "g_one", "tab-1", &first_id).expect("stale release"));
        assert!(validate(&home, "g_one", "tab-1", &second_id).is_ok());
        assert!(release(&home, "g_one", "tab-1", &second_id).expect("active release"));
    }
}
