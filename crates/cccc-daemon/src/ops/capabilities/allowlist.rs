use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::fs::{read_yaml, write_yaml};
use fs2::FileExt;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::fs::OpenOptions;
use std::io;

use crate::dispatch::{OpError, OpResult, object, string_arg};

const DEFAULT_TEXT: &str = include_str!("../../../resources/capability-allowlist.default.yaml");

pub(super) fn get(home: &HomeLayout) -> OpResult {
    object(snapshot(home)?)
}

pub(super) fn validate(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let overlay = next_overlay(home, request)?;
    let default = default_doc()?;
    let effective = merged(&default, &overlay);
    object(json!({
        "valid":true,
        "reason":"",
        "default":default,
        "overlay":overlay,
        "effective":effective,
        "revision":revision(&default,&overlay),
        "external_capability_safety_mode":safety_mode(&effective),
    }))
}

pub(super) fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if string_arg(request, "by").as_deref().unwrap_or("user") != "user" {
        return Err(OpError::new(
            "permission_denied",
            "only user can update capability allowlist overlay",
        ));
    }
    with_overlay_lock(home, || {
        let current = snapshot(home)?;
        let expected = string_arg(request, "expected_revision").unwrap_or_default();
        if !expected.is_empty() && current["revision"].as_str() != Some(&expected) {
            let mut error = OpError::new(
                "allowlist_revision_mismatch",
                "expected_revision does not match current revision",
            );
            error
                .details
                .insert("current_revision".into(), current["revision"].clone());
            return Err(error);
        }
        let overlay = next_overlay(home, request)?;
        persist_overlay(home, &overlay)?;
        let mut result = snapshot(home)?;
        result["updated"] = json!(true);
        object(result)
    })
}

pub(super) fn reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if string_arg(request, "by").as_deref().unwrap_or("user") != "user" {
        return Err(OpError::new(
            "permission_denied",
            "only user can reset capability allowlist overlay",
        ));
    }
    with_overlay_lock(home, || {
        let path = overlay_path(home);
        let removed = match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(OpError::io(error)),
        };
        let mut result = snapshot(home)?;
        result["reset"] = json!(true);
        result["removed_overlay_file"] = json!(removed);
        object(result)
    })
}

fn with_overlay_lock<T>(
    home: &HomeLayout,
    operation: impl FnOnce() -> Result<T, OpError>,
) -> Result<T, OpError> {
    let path = overlay_path(home).with_extension("yaml.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(OpError::io)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(OpError::io)?;
    file.lock_exclusive().map_err(OpError::io)?;
    let result = operation();
    let _ = FileExt::unlock(&file);
    result
}

fn persist_overlay(home: &HomeLayout, overlay: &Value) -> Result<(), OpError> {
    let path = overlay_path(home);
    if overlay.as_object().is_some_and(serde_json::Map::is_empty) {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(OpError::io(error)),
        }
    } else {
        write_yaml(&path, overlay).map_err(OpError::io)
    }
}

fn snapshot(home: &HomeLayout) -> Result<Value, OpError> {
    let default = default_doc()?;
    let overlay = load_overlay(home)?;
    let effective = merged(&default, &overlay);
    Ok(json!({
        "default":default,
        "overlay":overlay,
        "effective":effective,
        "revision":revision(&default,&overlay),
        "default_source":"builtin:capability-allowlist.default.yaml",
        "overlay_source":if overlay_path(home).exists(){overlay_path(home).to_string_lossy().into_owned()}else{String::new()},
        "overlay_error":"",
        "policy_source":"native",
        "policy_error":"",
        "external_capability_safety_mode":safety_mode(&effective),
    }))
}

fn next_overlay(home: &HomeLayout, request: &DaemonRequest) -> Result<Value, OpError> {
    let mode = string_arg(request, "mode").unwrap_or_else(|| "patch".into());
    match mode.as_str() {
        "replace" => request
            .args
            .get("overlay")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| {
                OpError::new(
                    "invalid_request",
                    "overlay must be an object when mode=replace",
                )
            }),
        "patch" => {
            let patch = request
                .args
                .get("patch")
                .filter(|value| value.is_object())
                .ok_or_else(|| {
                    OpError::new("invalid_request", "patch must be an object when mode=patch")
                })?;
            Ok(merged(&load_overlay(home)?, patch))
        }
        _ => Err(OpError::new(
            "invalid_request",
            "mode must be patch or replace",
        )),
    }
}

fn default_doc() -> Result<Value, OpError> {
    serde_yaml::from_str(DEFAULT_TEXT).map_err(OpError::invalid)
}

fn load_overlay(home: &HomeLayout) -> Result<Value, OpError> {
    let path = overlay_path(home);
    if path.exists() {
        let value = read_yaml::<Value>(&path).map_err(OpError::io)?;
        if value.is_object() {
            Ok(value)
        } else {
            Err(OpError::new(
                "allowlist_validation_failed",
                "allowlist YAML root must be a mapping",
            ))
        }
    } else {
        Ok(json!({}))
    }
}

fn overlay_path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join("config/capability-allowlist.user.yaml")
}

fn merged(base: &Value, overlay: &Value) -> Value {
    let mut output = base.clone();
    merge_value(&mut output, overlay);
    output
}

fn merge_value(target: &mut Value, patch: &Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            if let Some(existing) = target.get_mut(key)
                && existing.is_object()
                && value.is_object()
            {
                merge_value(existing, value);
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    } else {
        *target = patch.clone();
    }
}

fn revision(default: &Value, overlay: &Value) -> String {
    let payload = json!({"default":default,"overlay":overlay});
    format!("{:x}", Sha1::digest(python_json(&payload).as_bytes()))
}

fn python_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into()),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(python_json).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    serde_json::to_string(key).unwrap_or_else(|_| "\"\"".into()),
                    python_json(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn safety_mode(effective: &Value) -> &'static str {
    let source_levels = effective
        .get("defaults")
        .and_then(|value| value.get("source_level"))
        .and_then(Value::as_object);
    if source_levels
        .into_iter()
        .flatten()
        .any(|(_, level)| level.as_str().unwrap_or("mounted") != "indexed")
    {
        "normal"
    } else {
        "conservative"
    }
}

pub(super) fn effective_policy_level(
    home: &HomeLayout,
    capability_id: &str,
    kind: &str,
    source_id: &str,
    actor_role: &str,
) -> Result<String, OpError> {
    let effective = snapshot(home)?["effective"].clone();
    let mut level = effective
        .pointer(&format!(
            "/defaults/source_level/{}",
            escape_pointer(source_id)
        ))
        .and_then(Value::as_str)
        .map(normalize_level)
        .unwrap_or_else(|| {
            if source_id.is_empty() {
                "mounted".into()
            } else {
                "indexed".into()
            }
        });

    if kind == "skill" {
        for row in effective
            .pointer("/skills/source_overrides")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if row.get("source_id").and_then(Value::as_str) == Some(source_id)
                && let Some(value) = row.get("level").and_then(Value::as_str)
            {
                level = normalize_level(value);
            }
        }
    }

    for row in effective
        .get("mcp_overrides")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if row.get("capability_id").and_then(Value::as_str) == Some(capability_id)
            && let Some(value) = row.get("level").and_then(Value::as_str)
        {
            level = normalize_level(value);
        }
    }

    if kind == "skill"
        && let Some(skills) = effective.get("skills").and_then(Value::as_object)
    {
        for rows in skills.values().filter_map(Value::as_array) {
            for row in rows {
                if row.get("capability_id").and_then(Value::as_str) != Some(capability_id) {
                    continue;
                }
                if let Some(value) = row.get("level").and_then(Value::as_str) {
                    level = normalize_level(value);
                }
                if row
                    .get("pinned_roles")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|value| value.as_str() == Some(actor_role))
                {
                    level = "pinned".into();
                }
            }
        }
    }

    if !actor_role.is_empty()
        && effective
            .pointer(&format!(
                "/role_defaults/{}/pinned",
                escape_pointer(actor_role)
            ))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|value| value.as_str() == Some(capability_id))
    {
        level = "pinned".into();
    }
    Ok(level)
}

fn normalize_level(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "enabled" | "mounted" => "mounted".into(),
        "pinned" => "pinned".into(),
        _ => "indexed".into(),
    }
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
