use super::*;
use std::path::{Path, PathBuf};

pub(super) fn handle(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let action = string_arg(request, "action").unwrap_or_else(|| "run".into());
    let provider = provider(request);
    require_notebooklm(&provider)?;

    if action == "status" {
        return status(home, &group_id, &provider, &lane);
    }
    if action != "run" {
        return Err(OpError::new("invalid_args", "action must be status or run"));
    }
    Err(OpError::new(
        "capability_unavailable",
        "Automatic Group Space sync is retired; use explicit group_space_ingest and source operations",
    ))
}

fn status(home: &HomeLayout, group_id: &str, provider: &str, lane: &str) -> OpResult {
    let state = load(home, group_id)?;
    if lane == "work" {
        let sync = work_status_value(home, group_id, &state)?;
        return object(json!({"group_id":group_id,"provider":provider,"lane":"work","sync":sync}));
    }
    let (sync, summary) = memory_status_values(home, group_id, provider, &state)?;
    object(json!({
        "group_id":group_id,"provider":provider,"lane":"memory",
        "sync":sync,"summary":summary
    }))
}

pub(super) fn work_status_value(
    home: &HomeLayout,
    group_id: &str,
    state: &Value,
) -> Result<Value, OpError> {
    let Some(space_root) = work_space_root(home, group_id)? else {
        return Ok(json!({"available":false,"reason":"no_local_scope"}));
    };
    let mut sync = Value::Object(read_json_object(
        &space_root.join(".space-sync-state.json"),
    )?);
    sync["available"] = json!(true);
    sync["space_root"] = json!(space_root);
    let binding = &state["bindings"]["work"];
    let bound_remote_id = binding["remote_space_id"].as_str().unwrap_or_default();
    let binding_active = binding["status"].as_str() == Some("bound") && !bound_remote_id.is_empty();
    let stored_remote_id = sync["remote_space_id"].as_str().unwrap_or_default();
    if !binding_active {
        sync = neutral_work_sync(&space_root, "", "work_lane_unbound");
    } else if stored_remote_id != bound_remote_id {
        sync = neutral_work_sync(
            &space_root,
            bound_remote_id,
            if stored_remote_id.is_empty() {
                "sync_state_not_ready"
            } else {
                "binding_remote_mismatch"
            },
        );
    }
    Ok(sync)
}

pub(super) fn memory_status_values(
    home: &HomeLayout,
    group_id: &str,
    provider: &str,
    state: &Value,
) -> Result<(Value, Value), OpError> {
    let manifest_path = home
        .root()
        .join("groups")
        .join(group_id)
        .join("state/memory/notebooklm_sync.json");
    let binding = &state["bindings"]["memory"];
    let bound_remote_id = binding["remote_space_id"].as_str().unwrap_or_default();
    let mut sync = Value::Object(read_json_object(&manifest_path)?);
    let stored_remote_id = sync["remote_space_id"].as_str().unwrap_or_default();
    if bound_remote_id.is_empty()
        || (!stored_remote_id.is_empty() && stored_remote_id != bound_remote_id)
    {
        sync = json!({});
    }
    sync["v"] = json!(1);
    sync["provider"] = json!(provider);
    sync["lane"] = json!("memory");
    sync["group_id"] = json!(group_id);
    sync["remote_space_id"] = json!(bound_remote_id);
    sync["manifest_path"] = json!(manifest_path);
    if !sync["files"].is_object() {
        sync["files"] = json!({});
    }
    let summary = memory_summary(&sync, &manifest_path);
    Ok((sync, summary))
}

fn work_space_root(home: &HomeLayout, group_id: &str) -> Result<Option<PathBuf>, OpError> {
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(OpError::io)?;
    Ok(group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
        .map(|scope| Path::new(&scope.url).join("space")))
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, OpError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(OpError::io(error)),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(value)) => Ok(value),
        Ok(_) => Err(OpError::new(
            "invalid_state",
            format!("{} must contain a JSON object", path.display()),
        )),
        Err(error) => Err(OpError::new(
            "invalid_state",
            format!("failed to parse {}: {error}", path.display()),
        )),
    }
}

fn neutral_work_sync(space_root: &Path, remote_space_id: &str, reason: &str) -> Value {
    json!({
        "available":true,"reason":reason,"space_root":space_root,
        "remote_space_id":remote_space_id,"last_run_at":"","converged":false,
        "unsynced_count":0,"failed_count":0,"failed_items":[],"uploaded":0,
        "updated":0,"deleted":0,"reused":0,"remote_sources":0,
        "materialized_sources":0,"remote_artifacts":0,"downloaded_artifacts":0,
        "pruned_artifacts":0,"last_error":"","failure_signature":"",
        "last_fingerprint":{},"errors":[]
    })
}

fn memory_summary(sync: &Value, manifest_path: &Path) -> Value {
    let mut pending = 0_u64;
    let mut running = 0_u64;
    let mut failed = 0_u64;
    let mut blocked = 0_u64;
    let mut eligible = 0_u64;
    let mut synced = 0_u64;
    let mut empty = 0_u64;
    let mut last_eligible = "";
    let mut last_synced = "";
    for (date, item) in sync["files"].as_object().into_iter().flatten() {
        let state = item["state"].as_str().unwrap_or_default();
        match state {
            "pending" => pending += 1,
            "running" => running += 1,
            "failed" => failed += 1,
            "blocked" => blocked += 1,
            "skipped_empty" => empty += 1,
            _ => {}
        }
        if item["entry_count"].as_u64().unwrap_or(0) > 0 {
            eligible += 1;
            if date.as_str() > last_eligible {
                last_eligible = date;
            }
            if state == "succeeded" {
                synced += 1;
                if date.as_str() > last_synced {
                    last_synced = date;
                }
            }
        }
    }
    json!({
        "lane":"memory","manifest_path":manifest_path,
        "last_scan_at":sync.get("last_scan_at").cloned().unwrap_or(Value::Null),
        "last_success_at":sync.get("last_success_at").cloned().unwrap_or(Value::Null),
        "pending_files":pending,"running_files":running,"failed_files":failed,
        "blocked_files":blocked,"eligible_daily_files":eligible,
        "synced_daily_files":synced,"empty_daily_skipped":empty,
        "last_eligible_daily_date":if last_eligible.is_empty(){Value::Null}else{json!(last_eligible)},
        "last_synced_daily_date":if last_synced.is_empty(){Value::Null}else{json!(last_synced)}
    })
}
