use cccc_contracts::{DaemonRequest, Event};
use cccc_core::ledger;
use cccc_core::permissions;
use cccc_core::settings;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_settings_update" => group_settings(home, request),
        "observability_get" => global_get(home, "observability"),
        "observability_update" => global_update(home, request, "observability"),
        "branding_get" => global_get(home, "branding"),
        "branding_update" => global_update(home, request, "branding"),
        _ => return None,
    })
}

fn group_settings(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load_group(home, request)?;
    authorize(&group, request)?;
    let mut patch = patch(request)?;
    patch.remove("by");
    let settings_value = store(home)?
        .mutate(&group.group_id, |doc| {
            let value = doc.extra.entry("settings").or_insert_with(|| json!({}));
            let target = value
                .as_object_mut()
                .ok_or_else(|| std::io::Error::other("invalid group settings"))?;
            settings::merge(target, &patch);
            Ok(value.clone())
        })
        .map_err(OpError::io)?;
    append(
        home,
        &group.group_id,
        "group.settings_update",
        request,
        json!({"patch": patch}),
    )?;
    object(json!({"settings": settings_value}))
}

fn global_get(home: &HomeLayout, key: &str) -> OpResult {
    let global = settings::load(home).map_err(OpError::io)?;
    let value = if key == "branding" {
        json!(global.branding)
    } else {
        json!(global.observability)
    };
    object(json!({key: value}))
}

fn global_update(home: &HomeLayout, request: &DaemonRequest, key: &str) -> OpResult {
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if !by.is_empty() && by != "user" {
        return Err(OpError::new(
            "permission_denied",
            "only user can update global settings",
        ));
    }
    let patch = patch(request)?;
    let mut global = settings::load(home).map_err(OpError::io)?;
    let target = if key == "branding" {
        &mut global.branding
    } else {
        &mut global.observability
    };
    settings::merge(target, &patch);
    if key == "branding" {
        cccc_core::branding::touch(target);
    }
    let result = target.clone();
    settings::save(home, &global).map_err(OpError::io)?;
    object(json!({key: result}))
}

fn patch(request: &DaemonRequest) -> Result<Map<String, Value>, OpError> {
    request
        .args
        .get("patch")
        .and_then(Value::as_object)
        .cloned()
        .or_else(|| {
            request
                .args
                .get("settings")
                .and_then(Value::as_object)
                .cloned()
        })
        .ok_or_else(|| OpError::new("invalid_args", "patch must be an object"))
}

fn load_group(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)
}
fn authorize(group: &GroupDoc, request: &DaemonRequest) -> Result<(), OpError> {
    permissions::require_group(
        group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )
    .map_err(OpError::invalid)
}
fn append(
    home: &HomeLayout,
    group_id: &str,
    kind: &str,
    request: &DaemonRequest,
    data: Value,
) -> Result<(), OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)
}
