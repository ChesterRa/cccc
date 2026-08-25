use cccc_contracts::{ActorRole, DaemonRequest, Event};
use cccc_core::context::{ContextDoc, ContextStore};
use cccc_core::ledger;
use cccc_core::{GroupDoc, HomeLayout, actors};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, store, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "context_get" => get(home, request),
        "context_sync" => sync(home, request),
        "task_list" => super::task_list::run(home, request),
        _ => return None,
    })
}

fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    load_group(home, &group_id)?;
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let detail = string_arg(request, "detail").unwrap_or_else(|| "full".into());
    if !matches!(detail.as_str(), "overview" | "summary" | "full") {
        return Err(OpError::new(
            "invalid_detail",
            "detail must be 'overview', 'summary', or 'full'",
        ));
    }
    let document = if detail == "overview" {
        contexts.load_overview(&group_id)
    } else {
        contexts.load(&group_id)
    }
    .map_err(OpError::io)?;
    let version = contexts.version(&document).map_err(OpError::io)?;
    object(super::context_projection::project(
        document, version, &detail,
    ))
}

fn sync(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group = load_group(home, &group_id)?;
    let operations = parse_operations(request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let document = contexts.load(&group_id).map_err(OpError::io)?;
    authorize(&group, &document, &operations, &by)?;
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
        event.data = json!({"version": &result.version, "changes": &result.changes})
            .as_object()
            .cloned()
            .unwrap_or_default();
        ledger::append(
            &store(home)?.ledger_path(&group_id).map_err(OpError::io)?,
            &event,
        )
        .map_err(OpError::io)?;
    }
    object(json!({
        "success": true,
        "dry_run": result.dry_run,
        "changes": result.changes,
        "version": result.version,
    }))
}

pub(super) fn load_group(home: &HomeLayout, group_id: &str) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(group_id)
        .map_err(|_| OpError::new("group_not_found", format!("group not found: {group_id}")))
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

fn authorize(
    group: &GroupDoc,
    document: &ContextDoc,
    operations: &[Map<String, Value>],
    by: &str,
) -> Result<(), OpError> {
    if by.is_empty() || matches!(by, "user" | "system") {
        return Ok(());
    }
    let role = actors::effective_role(group, by).ok_or_else(|| {
        OpError::new(
            "permission_denied",
            format!("context changes require a known actor: {by}"),
        )
    })?;
    for operation in operations {
        let name = operation.get("op").and_then(Value::as_str).unwrap_or("");
        match name {
            "coordination.brief.update" | "meta.merge" if role != ActorRole::Foreman => {
                return Err(OpError::new(
                    "permission_denied",
                    format!("{name} requires foreman or user"),
                ));
            }
            "agent_state.update" | "agent_state.clear" => {
                if operation
                    .get("actor_id")
                    .and_then(Value::as_str)
                    .is_some_and(|actor_id| actor_id != by)
                {
                    return Err(OpError::new(
                        "permission_denied",
                        "actors may only update their own state",
                    ));
                }
            }
            "task.create" if role == ActorRole::Peer => {
                if operation
                    .get("assignee")
                    .and_then(Value::as_str)
                    .is_some_and(|assignee| !assignee.trim().is_empty() && assignee != by)
                {
                    return Err(OpError::new(
                        "permission_denied",
                        "peers may not create tasks assigned to another actor",
                    ));
                }
            }
            "task.update" | "task.move" | "task.restore" | "task.delete"
                if role == ActorRole::Peer =>
            {
                let Some(task_id) = operation
                    .get("task_id")
                    .and_then(Value::as_str)
                    .filter(|task_id| !task_id.trim().is_empty())
                else {
                    continue;
                };
                let Some(task) = document
                    .tasks
                    .iter()
                    .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id))
                else {
                    continue;
                };
                let owns_task = ["assignee", "handoff_to"]
                    .iter()
                    .any(|field| task.get(*field).and_then(Value::as_str) == Some(by));
                if !owns_task {
                    return Err(OpError::new(
                        "permission_denied",
                        format!("{name} requires the assignee, handoff target, foreman, or user"),
                    ));
                }
                if name == "task.update"
                    && operation
                        .get("assignee")
                        .and_then(Value::as_str)
                        .is_some_and(|assignee| !assignee.trim().is_empty() && assignee != by)
                {
                    return Err(OpError::new(
                        "permission_denied",
                        "peers may not reassign tasks to another actor",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
