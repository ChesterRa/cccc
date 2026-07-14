use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::capabilities::{Capability, CapabilityStore};
use cccc_core::settings;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "capability_overview" => overview(home),
        "capability_search" => search(home, request),
        "capability_enable" => enable(home, request),
        "capability_visibility" => visibility(home, request),
        "capability_block" => block(home, request),
        "capability_state" => state(home),
        "capability_import" => import(home, request),
        "capability_uninstall" => uninstall(home, request),
        "capability_install" => install(home, request),
        "capability_source_delete" => source_delete(home, request),
        "capability_tool_call" => use_capability(home, request),
        "capability_install_target" => {
            object(json!({"target": home.root().join("capabilities"), "runtime": "rust"}))
        }
        "capability_allowlist_get" => allowlist_get(home),
        "capability_allowlist_validate" => allowlist_validate(home, request),
        "capability_allowlist_update" => allowlist_update(home, request),
        "capability_allowlist_reset" => allowlist_reset(home),
        _ => return None,
    })
}

fn overview(home: &HomeLayout) -> OpResult {
    let store = CapabilityStore::new(home.clone());
    object(
        json!({"capabilities": store.catalog().map_err(OpError::io)?, "state": store.load().map_err(OpError::io)?}),
    )
}
fn search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let query = string_arg(request, "query").unwrap_or_default();
    object(
        json!({"capabilities": CapabilityStore::new(home.clone()).search(&query).map_err(OpError::io)?}),
    )
}
fn enable(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let state = CapabilityStore::new(home.clone())
        .set_enabled(&id, bool_arg(request, "enabled", true))
        .map_err(OpError::invalid)?;
    object(json!({"capability_id": id, "state": state}))
}
fn visibility(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let hidden = string_arg(request, "visibility").as_deref() == Some("hidden")
        || bool_arg(request, "hidden", false);
    let state = CapabilityStore::new(home.clone())
        .set_hidden(&id, hidden)
        .map_err(OpError::invalid)?;
    object(json!({"capability_id": id, "state": state}))
}
fn block(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let state = CapabilityStore::new(home.clone())
        .set_blocked(&id, bool_arg(request, "blocked", true))
        .map_err(OpError::invalid)?;
    object(json!({"capability_id": id, "state": state}))
}
fn state(home: &HomeLayout) -> OpResult {
    object(json!({"state": CapabilityStore::new(home.clone()).load().map_err(OpError::io)?}))
}
fn import(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let raw = request
        .args
        .get("capability")
        .cloned()
        .unwrap_or_else(|| Value::Object(request.args.clone()));
    let capability: Capability = serde_json::from_value(raw).map_err(OpError::invalid)?;
    let state = CapabilityStore::new(home.clone())
        .import(capability.clone())
        .map_err(OpError::invalid)?;
    object(json!({"capability": capability, "state": state}))
}
fn uninstall(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    object(
        json!({"capability_id": id, "removed": CapabilityStore::new(home.clone()).uninstall(&id).map_err(OpError::io)?}),
    )
}
fn install(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let target = required_arg(request, "target")?;
    let store = CapabilityStore::new(home.clone());
    let capability = store.require(&target).map_err(OpError::not_found)?;
    let state = store.set_enabled(&target, true).map_err(OpError::invalid)?;
    object(json!({"target":target,"capability":capability,"installed":true,"state":state}))
}
fn source_delete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = required_arg(request, "source_id")?;
    let removed = CapabilityStore::new(home.clone())
        .delete_source(&source)
        .map_err(OpError::io)?;
    object(json!({"source_id":source,"removed":removed,"deleted":true}))
}
fn use_capability(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let capability = CapabilityStore::new(home.clone())
        .require(&id)
        .map_err(OpError::not_found)?;
    object(json!({"capability": capability, "input": request.args.get("input"), "ready": true}))
}
fn allowlist_get(home: &HomeLayout) -> OpResult {
    object(json!({"allowlist": settings::load(home).map_err(OpError::io)?.capability_allowlist}))
}
fn allowlist_validate(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let values = ids(request)?;
    let catalog: std::collections::BTreeSet<_> = CapabilityStore::new(home.clone())
        .catalog()
        .map_err(OpError::io)?
        .into_iter()
        .map(|item| item.id)
        .collect();
    let unknown: Vec<_> = values
        .iter()
        .filter(|id| !catalog.contains(*id))
        .cloned()
        .collect();
    object(json!({"valid": unknown.is_empty(), "unknown": unknown}))
}
fn allowlist_update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let values = ids(request)?;
    let mut global = settings::load(home).map_err(OpError::io)?;
    global.capability_allowlist.clone_from(&values);
    settings::save(home, &global).map_err(OpError::io)?;
    object(json!({"allowlist": values}))
}
fn allowlist_reset(home: &HomeLayout) -> OpResult {
    let mut global = settings::load(home).map_err(OpError::io)?;
    global.capability_allowlist.clear();
    settings::save(home, &global).map_err(OpError::io)?;
    object(json!({"allowlist": []}))
}
fn ids(request: &DaemonRequest) -> Result<Vec<String>, OpError> {
    Ok(request
        .args
        .get("allowlist")
        .or_else(|| request.args.get("ids"))
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "allowlist must be an array"))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect())
}
