use cccc_contracts::{DaemonRequest, Event};
use cccc_core::context::ContextStore;
use cccc_core::ledger;
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, store, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "context_get" => get(home, request),
        "context_sync" => sync(home, request),
        "task_list" => task_list(home, request),
        _ => return None,
    })
}

fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    store(home)?.load(&group_id).map_err(OpError::not_found)?;
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let document = contexts.load(&group_id).map_err(OpError::io)?;
    let version = contexts.version(&document).map_err(OpError::io)?;
    let detail = string_arg(request, "detail").unwrap_or_else(|| "full".into());
    if !matches!(detail.as_str(), "summary" | "full") {
        return Err(OpError::new(
            "invalid_detail",
            "detail must be 'summary' or 'full'",
        ));
    }
    object(super::context_projection::project(
        document, version, &detail,
    ))
}

fn sync(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    let operations = parse_operations(request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    authorize(&group, &operations, &by)?;
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let result = contexts
        .sync(
            &group_id,
            &operations,
            string_arg(request, "if_version").as_deref(),
            &by,
            bool_arg(request, "dry_run", false),
        )
        .map_err(|error| {
            if error.to_string() == "version_conflict" {
                OpError::new("version_conflict", "context version conflict")
            } else {
                OpError::invalid(error)
            }
        })?;
    if !result.dry_run && !result.changes.is_empty() {
        let mut event = Event::new("context.sync", &group_id);
        event.by = by;
        event.data = json!({"version": result.version, "changes": result.changes})
            .as_object()
            .cloned()
            .unwrap_or_default();
        ledger::append(
            &store(home)?.ledger_path(&group_id).map_err(OpError::io)?,
            &event,
        )
        .map_err(OpError::io)?;
    }
    object(result)
}

fn task_list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let document = contexts.load(&group_id).map_err(OpError::io)?;
    let status = string_arg(request, "status");
    let tasks: Vec<_> = document
        .tasks
        .into_iter()
        .filter(|task| {
            status
                .as_deref()
                .is_none_or(|wanted| task.get("status").and_then(Value::as_str) == Some(wanted))
        })
        .collect();
    object(json!({"tasks": tasks}))
}

fn parse_operations(request: &DaemonRequest) -> Result<Vec<Map<String, Value>>, OpError> {
    request
        .args
        .get("ops")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "ops must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_object()
                .cloned()
                .ok_or_else(|| OpError::new("invalid_args", "each context op must be an object"))
        })
        .collect()
}

fn authorize(group: &GroupDoc, operations: &[Map<String, Value>], by: &str) -> Result<(), OpError> {
    if by.is_empty() || matches!(by, "user" | "system") {
        return Ok(());
    }
    for operation in operations {
        let name = operation.get("op").and_then(Value::as_str).unwrap_or("");
        match name {
            "coordination.brief.update" | "meta.merge" => {
                permissions::require_group(group, by).map_err(OpError::invalid)?;
            }
            "agent_state.update" | "agent_state.clear" => {
                if operation.get("actor_id").and_then(Value::as_str) != Some(by) {
                    return Err(OpError::new(
                        "permission_denied",
                        "actors may only update their own state",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
