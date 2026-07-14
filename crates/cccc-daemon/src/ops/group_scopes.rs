use cccc_contracts::DaemonRequest;
use cccc_core::active;
use cccc_core::group_scope;
use cccc_core::scope;
use cccc_core::{HomeLayout, Registry};
use serde_json::json;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "attach" => attach(home, request),
        "group_detach_scope" => detach(home, request),
        "group_use" => use_group(home, request),
        "registry_reconcile" => reconcile(home),
        _ => return None,
    })
}

fn attach(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let path = string_arg(request, "path").unwrap_or_else(|| ".".into());
    let detected = scope::detect(std::path::Path::new(&path)).map_err(OpError::invalid)?;
    let group = if let Some(id) = string_arg(request, "group_id").filter(|id| !id.is_empty()) {
        group_scope::attach(&store(home)?, &id, detected).map_err(OpError::io)?
    } else {
        let created = store(home)?
            .create(&detected.label, "")
            .map_err(OpError::io)?;
        group_scope::attach(&store(home)?, &created.group_id, detected).map_err(OpError::io)?
    };
    active::set(home, &group.group_id).map_err(OpError::io)?;
    object(json!({"group": group}))
}

fn detach(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let scope_key = required_arg(request, "scope_key")?;
    let group = group_scope::detach(&store(home)?, &group_id, &scope_key).map_err(OpError::io)?;
    object(json!({"group": group}))
}

fn use_group(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    let updated = if let Some(scope_key) =
        string_arg(request, "scope_key").filter(|value| !value.is_empty())
    {
        group_scope::activate(&store(home)?, &group_id, &scope_key).map_err(OpError::io)?
    } else {
        group
    };
    active::set(home, &updated.group_id).map_err(OpError::io)?;
    object(json!({"group": updated}))
}

fn reconcile(home: &HomeLayout) -> OpResult {
    let registry = Registry::load(home).map_err(OpError::io)?;
    object(json!({"groups": registry.groups, "reconciled": true}))
}
