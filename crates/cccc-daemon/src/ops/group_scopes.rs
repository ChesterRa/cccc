use cccc_contracts::DaemonRequest;
use cccc_core::active;
use cccc_core::group_scope;
use cccc_core::scope;
use cccc_core::{HomeLayout, Registry};
use serde_json::json;
use std::collections::BTreeSet;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "attach" => attach(home, request),
        "group_detach_scope" => detach(home, request),
        "group_use" => use_group(home, request),
        "registry_reconcile" => reconcile(home, request),
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

fn reconcile(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let registry = Registry::load(home).map_err(OpError::io)?;
    let scanned_groups = registry.groups.len();
    let mut missing_group_ids = Vec::new();
    let mut corrupt_group_ids = Vec::new();
    let group_store = store(home)?;
    for (group_id, meta) in &registry.groups {
        let document = std::path::Path::new(&meta.path).join("group.yaml");
        if !document.is_file() {
            missing_group_ids.push(group_id.clone());
        } else if group_store.load(group_id).is_err() {
            corrupt_group_ids.push(group_id.clone());
        }
    }
    let remove_missing = request
        .args
        .get("remove_missing")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mut removed_group_ids = Vec::new();
    let mut removed_default_scope_keys = Vec::new();
    if remove_missing && !missing_group_ids.is_empty() {
        let missing = missing_group_ids.iter().cloned().collect::<BTreeSet<_>>();
        Registry::mutate(home, |registry| {
            for group_id in &missing {
                if registry.groups.remove(group_id).is_some() {
                    removed_group_ids.push(group_id.clone());
                }
            }
            registry.defaults.retain(|scope_key, group_id| {
                if missing.contains(group_id) {
                    removed_default_scope_keys.push(scope_key.clone());
                    false
                } else {
                    true
                }
            });
            Ok(())
        })
        .map_err(OpError::io)?;
    }
    missing_group_ids.sort();
    corrupt_group_ids.sort();
    removed_group_ids.sort();
    removed_default_scope_keys.sort();
    object(json!({
        "dry_run":!remove_missing,
        "scanned_groups":scanned_groups,
        "missing_group_ids":missing_group_ids,
        "corrupt_group_ids":corrupt_group_ids,
        "removed_group_ids":removed_group_ids,
        "removed_default_scope_keys":removed_default_scope_keys,
    }))
}
