//! Administrator-only, synchronous source management. The durable fence wins
//! over partial Group cleanup; no provider operation occurs under these locks.
use crate::dispatch::{OpError, OpResult, object, required_arg};
use cccc_contracts::{DaemonRequest, source_control::SourceControl};
use cccc_core::{
    GroupStore, HomeLayout,
    local_sessions::{self, source_control as stored},
};
use serde_json::{Value, json};

pub(super) fn administrator(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.get("by").and_then(Value::as_str) != Some("user")
        || [
            "_cccc_web_binding",
            "_cccc_native_binding",
            "_cccc_native_source_binding",
        ]
        .iter()
        .any(|key| request.args.contains_key(*key))
    {
        return Err(OpError::new(
            "permission_denied",
            "Source management requires the local administrator",
        ));
    }
    Ok(())
}

pub(super) fn local_source(
    home: &HomeLayout,
    id: &str,
) -> Result<local_sessions::NativeSession, OpError> {
    if cccc_core::session_nodes::is_remote_source(id) {
        return Err(OpError::new(
            "source_control_unsupported",
            "Individual remote source control is unsupported; no device was revoked",
        ));
    }
    stored::state(home, id).map_err(OpError::invalid)?;
    local_sessions::read_record(home, id).map_err(OpError::not_found)
}

pub(super) fn control(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    administrator(request)?;
    let id = required_arg(request, "source_id")?;
    let action = required_arg(request, "action")?;
    if !matches!(action.as_str(), "remove" | "restore") {
        return Err(OpError::new("invalid_args", "Expected remove or restore"));
    }
    let expected = required_arg(request, "expected_revision")?;
    local_source(home, &id)?;
    stored::with_operation(home, &id, || {
        Ok(change(home, &id, action == "remove", &expected))
    })
    .map_err(OpError::io)?
}

fn result(id: &str, state: &SourceControl, partial: Option<&str>, failed: &[String]) -> OpResult {
    let mut value = serde_json::to_value(state).map_err(OpError::invalid)?;
    value["source_id"] = json!(id);
    value["client_requirement"] = json!("matching_build_and_reloaded_mcp");
    value["status"] = json!(if partial.is_some() {
        "partial"
    } else if state.access_blocked {
        "removed"
    } else {
        "active"
    });
    if let Some(code) = partial {
        value["error_code"] = json!(code);
    }
    value["failed_group_ids"] = json!(failed);
    object(value)
}

fn change(home: &HomeLayout, id: &str, remove: bool, expected: &str) -> OpResult {
    let mut state = stored::state(home, id).map_err(OpError::io)?;
    if expected != state.management_revision && remove != state.access_blocked {
        let mut error = OpError::new(
            "source_control_conflict",
            "Source management changed; read the current state before retrying",
        );
        error.details.insert("source_id".into(), json!(id));
        error.details.insert("current".into(), json!(state));
        return Err(error);
    }
    if !remove && !state.access_blocked && state.pause_complete {
        return result(id, &state, None, &[]);
    }
    if remove && !state.access_blocked {
        state.access_blocked = true;
        state.pause_complete = false;
        state.management_revision = uuid::Uuid::new_v4().to_string();
        stored::save(home, id, &state).map_err(OpError::io)?;
    }
    if state.access_blocked && state.pause_complete {
        let before = state.clone();
        state.pause_complete = false;
        if stored::save(home, id, &state).is_err() {
            let current = stored::state(home, id).unwrap_or(before);
            return result(id, &current, Some("source_control_storage_failed"), &[]);
        }
    }
    // Restore also scans and confirms every old membership before unblocking.
    // Do not roll this fence back if any registry/Group is unreadable.
    let failed = match pause_groups(home, id) {
        Ok(failed) => failed,
        Err(_) => {
            state.pause_complete = false;
            state.reason_code = Some("group_scan_failed".into());
            let _ = stored::save(home, id, &state);
            return result(id, &state, Some("group_scan_failed"), &[]);
        }
    };
    if !failed.is_empty() {
        state.pause_complete = false;
        state.reason_code = Some("group_pause_failed".into());
        let _ = stored::save(home, id, &state);
        return result(id, &state, Some("group_pause_failed"), &failed);
    }
    #[cfg(test)]
    concurrency_tests::after_pause(home);
    let before = state.clone();
    state.pause_complete = true;
    state.reason_code = None;
    if !remove {
        state.access_blocked = false;
        state.management_revision = uuid::Uuid::new_v4().to_string();
    }
    if stored::save(home, id, &state).is_err() {
        let current = stored::state(home, id).unwrap_or(before);
        return result(id, &current, Some("source_control_storage_failed"), &[]);
    }
    result(id, &state, None, &[])
}

fn pause_groups(home: &HomeLayout, id: &str) -> std::io::Result<Vec<String>> {
    let store = GroupStore::new(home.clone())?;
    let mut failed = Vec::new();
    for meta in store.list()? {
        let group = match store.load(&meta.group_id) {
            Ok(group) => group,
            Err(_) => {
                failed.push(meta.group_id);
                continue;
            }
        };
        if !group
            .actors
            .iter()
            .any(|actor| actor.external_session_id == id && actor.enabled)
        {
            continue;
        }
        if store
            .mutate(&group.group_id, |group| {
                for actor in group
                    .actors
                    .iter_mut()
                    .filter(|actor| actor.external_session_id == id)
                {
                    actor.enabled = false;
                }
                Ok(())
            })
            .is_err()
        {
            failed.push(group.group_id);
        }
    }
    Ok(failed)
}

pub(super) fn project(
    home: &HomeLayout,
    result: &mut serde_json::Map<String, Value>,
) -> Result<(), OpError> {
    let (records, _) = local_sessions::scan_records(home).map_err(OpError::io)?;
    let mut states = serde_json::Map::new();
    let mut removed = Vec::new();
    for record in records {
        let Ok(state) = stored::state(home, &record.source_id) else {
            // Leave the unavailable source out of the management projection.
            continue;
        };
        let diagnostic = if state.access_blocked {
            cccc_contracts::source_control::SourceDiagnostic::default()
        } else {
            super::diagnostics::cached(home, &record)
        };
        states.insert(
            record.source_id.clone(),
            json!({"control":state,"diagnostic":diagnostic}),
        );
        if state.access_blocked {
            removed.push(record.source(false));
        }
    }
    result.insert(
        "capabilities".into(),
        json!({"source_control":true,"source_diagnostics":true,"source_control_requires_mcp_reload":true}),
    );
    result.insert("source_states".into(), json!(states));
    result.insert("removed_sources".into(), json!(removed));
    Ok(())
}

#[cfg(test)]
#[path = "source_control_concurrency.rs"]
mod concurrency_tests;
