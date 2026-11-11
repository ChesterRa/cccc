//! Workspace Markdown remains authoritative; Context stores source links only.
mod git;
mod mutations;
mod promote;
mod state;
#[cfg(test)]
mod tests;

use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout, ledger, permissions, workspace};
use serde_json::{Value, json};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "backlog_get" => Operation::new(Read, get),
        "backlog_use" | "backlog_create" | "backlog_exclude" => {
            Operation::new(Write, mutations::configure)
        }
        "backlog_add" | "backlog_edit" | "backlog_move" | "backlog_close" => {
            Operation::new(Write, mutations::edit)
        }
        "backlog_promote" => Operation::new(Write, promote::run),
        _ => return None,
    })
}

fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let Some(group) = state::scoped(home, request, false)? else {
        return object(state::empty("", ""));
    };
    state::snapshot(home, &group)
}

fn by(request: &DaemonRequest) -> String {
    string_arg(request, "by").unwrap_or_else(|| "user".into())
}

fn setting(group: &GroupDoc) -> Option<&str> {
    group
        .extra
        .get("backlogs")?
        .get(&group.active_scope_key)?
        .get("path")?
        .as_str()
}

fn path(group: &GroupDoc) -> Result<String, OpError> {
    setting(group)
        .map(str::to_owned)
        .ok_or_else(|| OpError::new("backlog_not_configured", "Choose a backlog file first"))
}

fn selected_path(group: &GroupDoc, request: &DaemonRequest) -> Result<String, OpError> {
    let path = path(group)?;
    if required_arg(request, "path")? != path {
        return Err(OpError::new(
            "backlog_file_conflict",
            "Selected backlog file changed; reload before writing",
        ));
    }
    Ok(path)
}

fn event(
    home: &HomeLayout,
    group: &GroupDoc,
    request: &DaemonRequest,
    op: &str,
    path: &str,
    sha: &str,
    id: Option<&str>,
) -> OpResult {
    let mut change = json!({"op":op, "path":path});
    if let Some(id) = id {
        change["entry_id"] = json!(id);
    }
    let mut event = Event::new("context.sync", &group.group_id);
    event.by = by(request);
    event.scope_key.clone_from(&group.active_scope_key);
    event.data = object(
        json!({"changes":[change], "backlog":{"scope_key":group.active_scope_key,"path":path,"sha256":sha}}),
    )?;
    ledger::append(
        &store(home)?
            .ledger_path(&group.group_id)
            .map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    object(json!({"event_id":event.id}))
}

fn partial(error: OpError, applied: Value) -> OpError {
    let mut failure = OpError::new(
        "backlog_partial_failure",
        format!("{}; reload before retrying", error.message),
    );
    failure.details = object(json!({"applied":applied,"next":"reload","cause_code":error.code}))
        .unwrap_or_default();
    failure
}

fn file_applied(path: &str, sha: &str, id: Option<&str>) -> Value {
    let mut result = json!({"file":{"path":path,"sha256":sha}});
    if let Some(id) = id {
        result["file"]["entry_id"] = json!(id);
    }
    result
}

fn file_error(error: std::io::Error) -> OpError {
    if error
        .get_ref()
        .is_some_and(|cause| cause.is::<workspace::OutsideScope>())
    {
        OpError::new("permission_denied", error.to_string())
    } else if error.kind() == std::io::ErrorKind::NotFound {
        OpError::new(
            "backlog_not_found",
            "Backlog file or its parent folder no longer exists",
        )
    } else {
        OpError::new("backlog_invalid_file", error.to_string())
    }
}

fn normalized_path(group: &GroupDoc, value: &str) -> Result<String, OpError> {
    workspace::safe_relative(value).map_err(file_error)?;
    if value.contains(['\\', '\n', '\r', '\0']) {
        return Err(OpError::invalid("path must be a portable relative path"));
    }
    let root = workspace::root(group).map_err(file_error)?;
    let candidate = match workspace::resolve_file(group, value) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            workspace::resolve_for_create(&root, value).map_err(file_error)?
        }
        Err(error) => return Err(file_error(error)),
    };
    let relative = candidate
        .strip_prefix(root)
        .map_err(OpError::invalid)?
        .to_str()
        .ok_or_else(|| OpError::invalid("path must be UTF-8"))?
        .replace(std::path::MAIN_SEPARATOR, "/");
    let source = cccc_contracts::backlog::BacklogSource {
        scope_key: group.active_scope_key.clone(),
        path: relative.clone(),
        entry_id: "000000000000".into(),
    };
    source.validate().map_err(OpError::invalid)?;
    Ok(relative)
}
