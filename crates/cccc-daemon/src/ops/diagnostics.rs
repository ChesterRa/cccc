use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, settings};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::actor_runtime;

mod tail;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "debug_snapshot" => snapshot(home, request),
        "debug_tail_logs" => tail_logs(home, request),
        "debug_clear_logs" => clear_logs(home, request),
        _ => return None,
    })
}

fn snapshot(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_developer_mode(home)?;
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    let global = settings::load(home).map_err(OpError::io)?;
    let mut result = json!({
        "implementation":"rust",
        "version":env!("CARGO_PKG_VERSION"),
        "pid":std::process::id(),
        "home":home.root(),
        "observability":global.observability,
    });
    if !group_id.is_empty() {
        let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
        let actors = group
            .actors
            .iter()
            .map(|actor| {
                let status = actor_runtime::status(&group_id, &actor.id);
                json!({
                    "id":actor.id,
                    "role":actor.role,
                    "runtime":actor.runtime,
                    "runner":actor.runner,
                    "enabled":actor.enabled,
                    "running":status.as_ref().is_some_and(|status| status.running),
                    "pid":status.and_then(|status| status.pid)
                })
            })
            .collect::<Vec<_>>();
        result["group"] = json!({
            "group_id":group.group_id,
            "title":group.title,
            "state":group.state,
            "running":group.running,
            "active_scope_key":group.active_scope_key
        });
        result["actors"] = Value::Array(actors);
    }
    object(result)
}

fn tail_logs(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_developer_mode(home)?;
    let component = required_arg(request, "component")?.to_ascii_lowercase();
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    let lines = request
        .args
        .get("lines")
        .and_then(Value::as_u64)
        .unwrap_or(200)
        .clamp(1, 2_000) as usize;
    let path = log_path(home, &component, &group_id)?;
    let output = if path.exists() {
        tail::read_last_lines(&path, lines).map_err(OpError::io)?
    } else {
        Vec::new()
    };
    object(json!({"component":component,"group_id":group_id,"path":path,"lines":output}))
}

fn clear_logs(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_developer_mode(home)?;
    let component = required_arg(request, "component")?.to_ascii_lowercase();
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    let path = log_path(home, &component, &group_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(OpError::io)?;
    }
    fs::write(&path, []).map_err(OpError::io)?;
    object(json!({"component":component,"group_id":group_id,"path":path,"cleared":true}))
}

fn require_developer_mode(home: &HomeLayout) -> Result<(), OpError> {
    let settings = settings::load(home).map_err(OpError::io)?;
    if settings
        .observability
        .get("developer_mode")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(OpError::new(
            "developer_mode_required",
            "developer mode is disabled",
        ))
    }
}

fn log_path(home: &HomeLayout, component: &str, group_id: &str) -> Result<PathBuf, OpError> {
    match component {
        "daemon" | "ccccd" => Ok(home.daemon_dir().join("ccccd.log")),
        "web" => Ok(home.daemon_dir().join("cccc-web.log")),
        "im" | "im_bridge" => {
            if group_id.is_empty() {
                return Err(OpError::new(
                    "invalid_args",
                    "group_id is required for IM logs",
                ));
            }
            Ok(GroupStore::new(home.clone())
                .map_err(OpError::io)?
                .state_dir(group_id)
                .map_err(OpError::io)?
                .join("im_bridge.log"))
        }
        _ => Err(OpError::new("invalid_args", "unknown debug component")),
    }
}
