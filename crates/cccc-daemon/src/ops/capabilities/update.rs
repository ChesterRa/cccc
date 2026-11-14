use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::HomeLayout;
use cccc_core::capabilities::CapabilityStore;
use cccc_core::fs::{read_json, with_exclusive_lock, write_json};
use serde_json::{Value, json};
use std::collections::BTreeSet;

use crate::dispatch::{OpError, OpResult, object, required_arg};

#[cfg(test)]
mod tests;

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let user = match request.args.get("by") {
        None => true,
        Some(Value::String(value)) => value == "user",
        _ => false,
    };
    if !user {
        return Err(OpError::new(
            "permission_denied",
            "only user can update a capability installation",
        ));
    }
    let id = required_arg(request, "capability_id")?;
    let expected = required_arg(request, "expected_recipe_fingerprint")?;
    let store = CapabilityStore::new(home.clone());
    let record = store
        .resolved_record(&id)
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("capability_not_found", "capability record not found"))?;
    let fingerprint = super::recipe::fingerprint(&record)?;
    if expected != fingerprint {
        return Err(OpError::new(
            "capability_recipe_conflict",
            "execution recipe changed; review the current recipe before updating",
        ));
    }
    let installed = super::package_install::installed_artifact(home, &id)?.ok_or_else(|| {
        OpError::new(
            "capability_not_installed",
            "enable the capability before updating its installation",
        )
    })?;
    let count = enabled_group_count(home, &id)?;
    if recorded_fingerprint(&installed).as_deref() == Some(&fingerprint) {
        return object(
            json!({"capability_id":id,"updated":false,"state":"unchanged",
            "artifact_id":installed["artifact_id"],"recipe_fingerprint":fingerprint,
            "scope":"global","enabled_group_count":count,"refresh_required":false}),
        );
    }
    let runtime: Value =
        read_json(&home.root().join("state/capabilities/runtime.json")).map_err(OpError::io)?;
    if !update_supported(&record, &installed) || member_authentication(&runtime, &id) {
        return Err(OpError::new(
            "capability_update_unsupported",
            "update requires a supported public recipe without legacy inline credentials",
        ));
    }
    let candidate = super::package_install::prepare(&id, &record)?;
    commit(home, &id, &candidate)?;
    object(
        json!({"capability_id":id,"updated":true,"state":if super::credentials::bearer(&record){"configured"}else{"ready"},
        "previous_artifact_id":installed["artifact_id"],"artifact_id":candidate["artifact_id"],
        "recipe_fingerprint":fingerprint,"scope":"global","enabled_group_count":count,
        "refresh_required":true,"refresh_mode":"relist_or_reconnect"}),
    )
}

pub(super) fn recorded_fingerprint(artifact: &Value) -> Option<String> {
    if let Some(value) = artifact["recipe_fingerprint"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        return Some(value.into());
    }
    // Only a stored snapshot can describe an old recipe. Never substitute the
    // current catalog and accidentally declare an unknown installation current.
    artifact
        .get("install_spec")
        .filter(|spec| spec.is_object())
        .and_then(|spec| {
            super::recipe::fingerprint(&json!({"install_mode":artifact["install_mode"],
            "install_spec":spec,"required_env":artifact.get("required_env")}))
            .ok()
        })
}

pub(super) fn enabled_group_count(home: &HomeLayout, id: &str) -> Result<usize, OpError> {
    let path = home.root().join("state/capabilities/state.json");
    if !path.exists() {
        return Ok(0);
    }
    let state: Value = read_json(&path).map_err(OpError::io)?;
    let existing = cccc_core::GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .list()
        .map_err(OpError::io)?
        .into_iter()
        .map(|group| group.group_id)
        .collect::<BTreeSet<_>>();
    Ok(count_groups(&state, &existing, id))
}

pub(super) fn info(
    record: &Value,
    id: &str,
    runtime: &Value,
    controls: &Value,
    existing: &BTreeSet<String>,
) -> Value {
    let candidate = super::recipe::fingerprint(record).ok();
    let installed = runtime["capability_artifacts"][id]
        .as_str()
        .and_then(|artifact_id| runtime["artifacts"].get(artifact_id))
        .filter(|artifact| {
            matches!(
                artifact["state"].as_str(),
                Some("installed" | "ready" | "active")
            )
        });
    let recorded = installed.and_then(recorded_fingerprint);
    let reason = if installed.is_none() {
        "not_installed"
    } else if recorded.is_none() {
        "recipe_unknown"
    } else if candidate != recorded {
        "recipe_changed"
    } else {
        "current"
    };
    json!({"available":installed.is_some_and(|artifact|update_supported(record,artifact)) && !member_authentication(runtime,id) && candidate.is_some() && recorded!=candidate,
        "scope":"global","enabled_group_count":count_groups(controls,existing,id),
        "recipe_fingerprint":candidate,"installed_recipe_fingerprint":recorded,"reason":reason,
        "unavailable_reason":if installed.is_some() && (!update_supported(record,installed.unwrap_or(&Value::Null)) || member_authentication(runtime,id)) {
            json!(if record.pointer("/support/auth").and_then(Value::as_str).is_some_and(|auth|matches!(auth,"oauth"|"api_key"|"account")) {"delivery_form_not_supported"}else{"credential_route_not_supported"})
        }else{Value::Null}})
}

fn member_authentication(runtime: &Value, id: &str) -> bool {
    runtime
        .get("actor_instances")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .flat_map(|(_, actors)| actors.as_object().into_iter().flatten())
        .filter_map(|(_, bindings)| bindings[id]["artifact_id"].as_str())
        .any(|artifact_id| {
            runtime["artifacts"][artifact_id]
                .pointer("/invoker/token")
                .and_then(Value::as_str)
                .is_some_and(|token| !token.is_empty())
        })
}

fn update_supported(record: &Value, installed: &Value) -> bool {
    let auth = record
        .pointer("/support/auth")
        .and_then(Value::as_str)
        .unwrap_or("none");
    (auth == "none"
        || (auth == "bearer"
            && record["install_mode"] == "remote_only"
            && super::credentials::env_name(record).is_some()))
        && installed
            .pointer("/invoker/token")
            .and_then(Value::as_str)
            .is_none_or(|token| token.is_empty())
}

fn count_groups(state: &Value, existing: &BTreeSet<String>, id: &str) -> usize {
    let mut groups = BTreeSet::new();
    for key in ["group_enabled", "actor_enabled", "session_enabled"] {
        for (group, entries) in state
            .get(key)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if !existing.contains(group) {
                continue;
            }
            let active = if key == "group_enabled" {
                has_id(entries, id, false)
            } else {
                entries
                    .as_object()
                    .into_iter()
                    .flatten()
                    .any(|(_, items)| has_id(items, id, key == "session_enabled"))
            };
            if active {
                groups.insert(group);
            }
        }
    }
    groups.len()
}

fn has_id(items: &Value, id: &str, session: bool) -> bool {
    items.as_array().into_iter().flatten().any(|item| {
        if !session {
            return item.as_str() == Some(id);
        }
        item["capability_id"] == id
            && item["expires_at"]
                .as_str()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .is_some_and(|expires| expires > chrono::Utc::now())
    })
}

fn commit(home: &HomeLayout, id: &str, candidate: &Value) -> Result<(), OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut state: Value = read_json(&path)?;
        let candidate_id = candidate["artifact_id"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("candidate artifact id is missing"))?;
        let mut superseded = BTreeSet::new();
        if let Some(old) = state["capability_artifacts"][id].as_str() {
            superseded.insert(old.to_owned());
        }
        state["artifacts"][candidate_id] = candidate.clone();
        state["capability_artifacts"][id] = json!(candidate_id);
        for (_, actors) in state
            .get_mut("actor_instances")
            .and_then(Value::as_object_mut)
            .into_iter()
            .flatten()
        {
            for (_, bindings) in actors.as_object_mut().into_iter().flatten() {
                if let Some(binding) = bindings.get_mut(id).and_then(Value::as_object_mut) {
                    if let Some(old) = binding.get("artifact_id").and_then(Value::as_str) {
                        superseded.insert(old.to_owned());
                    }
                    binding.insert("artifact_id".into(), json!(candidate_id));
                    binding.insert(
                        "state".into(),
                        json!(if candidate.pointer("/invoker/credential").is_some() {
                            "activation_pending"
                        } else {
                            "runnable"
                        }),
                    );
                    binding.insert("last_error".into(), json!(""));
                    binding.insert("updated_at".into(), json!(utc_now()));
                }
            }
        }
        for old in superseded {
            if old != candidate_id && !referenced(&state, &old) {
                if let Some(artifacts) = state.get_mut("artifacts").and_then(Value::as_object_mut) {
                    artifacts.remove(&old);
                }
            }
        }
        state["updated_at"] = json!(utc_now());
        write_json(&path, &state)
    })
    .map_err(OpError::io)
}

fn referenced(state: &Value, artifact_id: &str) -> bool {
    state
        .get("capability_artifacts")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .any(|(_, value)| value.as_str() == Some(artifact_id))
        || state
            .get("actor_instances")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .flat_map(|(_, actors)| actors.as_object().into_iter().flatten())
            .flat_map(|(_, items)| items.as_object().into_iter().flatten())
            .any(|(_, binding)| binding["artifact_id"].as_str() == Some(artifact_id))
}
