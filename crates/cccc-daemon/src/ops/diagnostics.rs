use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout, settings};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
mod tail;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "debug_snapshot" => Operation::new(Read, snapshot),
        "debug_tail_logs" => Operation::new(Read, tail_logs),
        "debug_clear_logs" => Operation::new(Write, clear_logs),
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
        "build":cccc_core::build_info::current(),
        "executable":std::env::current_exe().ok(),
        "pid":std::process::id(),
        "home":home.root(),
        "observability":global.observability,
        "memory":memory_snapshot(),
    });
    if !group_id.is_empty() {
        let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
        let actors = group
            .actors
            .iter()
            .map(|actor| {
                let status = crate::ops::actor_runtime_status::resolve(home, &group, actor);
                json!({
                    "id":actor.id,
                    "role":actor.role,
                    "runtime":actor.runtime,
                    "runner":actor.runner,
                    "enabled":actor.enabled,
                    "running":if status.available { Some(status.running) } else { None },
                    "runtime_owner_available":status.available,
                    "pid":status.pid
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

/// Diagnostic memory view. Ledger index figures are eviction budgets, not
/// measured memory; compare them with the process figures from the same read.
fn memory_snapshot() -> Value {
    let index = cccc_core::ledger_index::stats();
    let (rss, anon, swap) = process_status_kib();
    json!({
        "ledger_index":{
            "entries":index.entries,
            "budget_bytes":index.budget_bytes,
            "budget_limit_bytes":index.budget_limit_bytes,
            "largest_budget_bytes":index.largest_budget_bytes,
            "pinned":index.pinned,
            "hits":index.hits,
            "entries_created":index.entries_created,
            "index_rebuilds":index.index_rebuilds,
            "evictions":index.evictions,
            "invalidations":index.invalidations,
        },
        "process":{"vm_rss_kib":rss,"rss_anon_kib":anon,"vm_swap_kib":swap},
    })
}

/// VmRSS, RssAnon and VmSwap from one read of /proc/self/status; None where
/// the platform or kernel does not report a field.
fn process_status_kib() -> (Option<u64>, Option<u64>, Option<u64>) {
    let Ok(status) = fs::read_to_string("/proc/self/status") else {
        return (None, None, None);
    };
    let field = |name: &str| {
        status.lines().find_map(|line| {
            let mut fields = line.split_ascii_whitespace();
            (fields.next()? == name).then_some(())?;
            let value = fields.next()?.parse().ok()?;
            (fields.next()? == "kB").then_some(value)
        })
    };
    (field("VmRSS:"), field("RssAnon:"), field("VmSwap:"))
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
    let output = if component == "daemon_diagnostics" {
        let previous = home.daemon_dir().join(crate::diagnostic_log::PREVIOUS);
        if !path.exists() && !previous.exists() {
            Vec::new()
        } else {
            crate::diagnostic_log::with_lock(&home.daemon_dir(), false, || {
                let mut output = if previous.exists() {
                    tail::read_last_lines(&previous, lines)?
                } else {
                    Vec::new()
                };
                if path.exists() {
                    output.extend(tail::read_last_lines(&path, lines)?);
                }
                let extra = output.len().saturating_sub(lines);
                output.drain(..extra);
                Ok(output)
            })
            .map_err(OpError::io)?
        }
    } else if path.exists() {
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
    if component == "daemon_diagnostics" {
        crate::diagnostic_log::clear(&home.daemon_dir()).map_err(OpError::io)?;
    } else {
        fs::write(&path, []).map_err(OpError::io)?;
    }
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
        "daemon_diagnostics" => Ok(home.daemon_dir().join(crate::diagnostic_log::NAME)),
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
