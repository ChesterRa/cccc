use cccc_contracts::{DaemonRequest, Event, GroupState};
use cccc_core::active;
use cccc_core::group_prompts::{
    DEFAULT_PREAMBLE_BODY, PREAMBLE_FILENAME, delete_preamble, read_preamble, write_preamble,
};
use cccc_core::ledger;
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout, group_bridge_legacy, integration_state};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{actor_delivery, actor_runtime, group_runtime};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_create" => create(home, request),
        "group_list" | "groups" => list(home),
        "group_show" => show(home, request),
        "group_preamble_get" => preamble_get(home, request),
        "group_preamble_set" => preamble_set(home, request),
        "group_preamble_reset" => preamble_reset(home, request),
        "group_resolve" => resolve(home, request),
        "group_update" => update(home, request),
        "group_delete" => delete(home, request),
        "group_reset" => super::group_reset::reset(home, request),
        "group_set_state" => set_state(home, request),
        "group_start" => running(home, request, true),
        "group_stop" => running(home, request, false),
        _ => return None,
    })
}

fn resolve(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let raw = required_arg(request, "token")?;
    let token = raw.trim().trim_start_matches('#').to_ascii_lowercase();
    let store = store(home)?;
    let matches = store
        .list()
        .map_err(OpError::io)?
        .into_iter()
        .filter_map(|meta| store.load(&meta.group_id).ok())
        .filter_map(|group| {
            let matched_by = if group.group_id.to_ascii_lowercase() == token {
                "group_id"
            } else if group.title.trim().to_ascii_lowercase() == token {
                "title"
            } else if group.topic.trim().to_ascii_lowercase() == token {
                "topic"
            } else {
                return None;
            };
            Some(json!({
                "group_id":group.group_id,"title":group.title,"topic":group.topic,
                "running":group.running,"state":group.state,"matched_by":matched_by,"token":raw
            }))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [item] => object(item.clone()),
        [] => resolve_remote(home, request, &token, &raw),
        _ => {
            let mut error =
                OpError::new("ambiguous", format!("multiple groups match token: {raw}"));
            error.details.insert("candidates".into(), json!(matches));
            Err(error)
        }
    }
}

fn resolve_remote(home: &HomeLayout, request: &DaemonRequest, token: &str, raw: &str) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    group_bridge_legacy::import_if_changed(home).map_err(OpError::io)?;
    let state = integration_state::global_get(home, "group_bridge").map_err(OpError::io)?;
    let route = state
        .get("trusts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|item| {
            item["status"] == "active"
                && item["group_id"] == group_id
                && super::group_bridge::route_ready(home, item)
                && [item["remote_group_id"].as_str(), item["remote_group_title"].as_str()]
                    .into_iter()
                    .flatten()
                    .map(|value| value.trim().trim_start_matches('#').to_ascii_lowercase())
                    .any(|value| value == token)
        })
        .ok_or_else(|| {
            OpError::new(
                "not_found",
                format!(
                    "no group matches token: {raw}; inspect group list or trusted Group Bridge routes"
                ),
            )
        })?;
    let remote_group_id = route["remote_group_id"].as_str().unwrap_or("");
    object(json!({
        "group_id":remote_group_id,
        "title":route["remote_group_title"].as_str().filter(|value|!value.is_empty())
            .unwrap_or(remote_group_id),
        "topic":"","running":true,"state":"active",
        "matched_by":"group_bridge_remote_group_title","token":raw,
        "group_bridge":true,"registration_id":route["registration_id"],
        "trust_id":route["trust_id"]
    }))
}

fn create(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let title = string_arg(request, "title").unwrap_or_else(|| "working-group".into());
    let topic = string_arg(request, "topic").unwrap_or_default();
    let group_store = store(home)?;
    let previous_active = active::get(home).map_err(OpError::io)?;
    let group = group_store.create(&title, &topic).map_err(OpError::io)?;
    if let Err(error) = append_group_event(
        home,
        &group,
        "group.create",
        request,
        json!({"title": group.title, "topic": group.topic}),
    ) {
        return Err(super::group_create_rollback::run(
            home,
            &group_store,
            &group.group_id,
            None,
            error,
        ));
    }
    if let Err(error) = active::set(home, &group.group_id).map_err(OpError::io) {
        return Err(super::group_create_rollback::run(
            home,
            &group_store,
            &group.group_id,
            Some(previous_active),
            error,
        ));
    }
    object(json!({"group_id": group.group_id, "group": group_runtime::group(group)}))
}

fn list(home: &HomeLayout) -> OpResult {
    let store = store(home)?;
    let groups = store
        .list()
        .map_err(OpError::io)?
        .into_iter()
        .filter_map(|meta| {
            store
                .load(&meta.group_id)
                .ok()
                .map(|group| group_runtime::summary(meta, &group))
        })
        .collect::<Vec<_>>();
    object(json!({"groups": groups}))
}

fn show(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    object(json!({"group": group_runtime::group(load(home, request)?)}))
}

fn preamble_get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = preamble_group_id(request)?;
    let group = load_preamble_group(home, &group_id)?;
    preamble_result(home, &group.group_id, None)
}

fn preamble_set(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = preamble_group_id(request)?;
    let content = string_arg(request, "content").filter(|value| !value.trim().is_empty());
    let Some(content) = content else {
        return Err(OpError::new(
            "invalid_content",
            "group preamble content must be a non-empty string; use group_preamble_reset to restore the builtin",
        ));
    };
    let group = load_preamble_group(home, &group_id)?;
    authorize(&group, request)
        .map_err(|error| OpError::new("group_preamble_set_failed", error.message))?;
    let store =
        store(home).map_err(|error| OpError::new("group_preamble_set_failed", error.message))?;
    let current = read_preamble(&store, &group.group_id)
        .map_err(|error| OpError::new("group_preamble_set_failed", error.to_string()))?;
    let changed = !current.found || current.content.as_deref() != Some(content.as_str());
    if changed {
        write_preamble(&store, &group.group_id, &content)
            .map_err(|error| OpError::new("group_preamble_set_failed", error.to_string()))?;
    }
    preamble_result(home, &group.group_id, Some(changed))
}

fn preamble_reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = preamble_group_id(request)?;
    if !string_arg(request, "confirm")
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("preamble")
    {
        return Err(OpError::new(
            "confirm_required",
            "confirm must equal preamble",
        ));
    }
    let group = load_preamble_group(home, &group_id)?;
    authorize(&group, request)
        .map_err(|error| OpError::new("group_preamble_reset_failed", error.message))?;
    let store =
        store(home).map_err(|error| OpError::new("group_preamble_reset_failed", error.message))?;
    let changed = read_preamble(&store, &group.group_id)
        .map_err(|error| OpError::new("group_preamble_reset_failed", error.to_string()))?
        .found;
    delete_preamble(&store, &group.group_id)
        .map_err(|error| OpError::new("group_preamble_reset_failed", error.to_string()))?;
    preamble_result(home, &group.group_id, Some(changed))
}

fn preamble_group_id(request: &DaemonRequest) -> Result<String, OpError> {
    string_arg(request, "group_id")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_owned())
        .ok_or_else(|| OpError::new("missing_group_id", "missing group_id"))
}

fn load_preamble_group(home: &HomeLayout, group_id: &str) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(group_id)
        .map_err(|_| OpError::new("group_not_found", format!("group not found: {group_id}")))
}

fn preamble_result(home: &HomeLayout, group_id: &str, changed: Option<bool>) -> OpResult {
    let prompt = read_preamble(&store(home)?, group_id).map_err(OpError::io)?;
    let override_content = prompt.content.unwrap_or_default();
    let overridden = prompt.found && !override_content.trim().is_empty();
    let mut result = json!({
        "group_id": group_id,
        "source": if overridden { "home" } else { "builtin" },
        "filename": PREAMBLE_FILENAME,
        "overridden": overridden,
        "content": if overridden {
            override_content
        } else {
            DEFAULT_PREAMBLE_BODY.trim().to_owned()
        },
    });
    if let Some(changed) = changed {
        result["changed"] = json!(changed);
    }
    object(result)
}

fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let current = load(home, request)?;
    authorize(&current, request)?;
    let title = string_arg(request, "title");
    let topic = string_arg(request, "topic");
    if title.is_none() && topic.is_none() {
        return Err(OpError::new("invalid_args", "title or topic is required"));
    }
    let group = store(home)?
        .update(&current.group_id, title.as_deref(), topic.as_deref())
        .map_err(OpError::not_found)?;
    append_group_event(
        home,
        &group,
        "group.update",
        request,
        json!({"patch": {"title": title, "topic": topic}}),
    )?;
    object(json!({"group": group_runtime::group(group)}))
}

fn delete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    actor_delivery::shutdown_group(&group.group_id);
    actor_runtime::stop_group(&group)?;
    let deleted = store(home)?.delete(&group.group_id).map_err(OpError::io)?;
    if deleted {
        super::actor_secrets::remove_group(home, &group.group_id)?;
    }
    if active::get(home).map_err(OpError::io)?.as_deref() == Some(&group.group_id) {
        active::clear(home).map_err(OpError::io)?;
    }
    object(json!({"group_id": group.group_id, "deleted": deleted}))
}

fn set_state(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    let raw = required_arg(request, "state")?;
    let state: GroupState = serde_json::from_value(Value::String(raw)).map_err(OpError::invalid)?;
    if matches!(state, GroupState::Paused | GroupState::Stopped) {
        actor_delivery::shutdown_group(&group.group_id);
        super::local_headless::stop_group(&group.group_id);
    }
    let updated = store(home)?
        .mutate(&group.group_id, |doc| {
            doc.state = state;
            Ok(doc.clone())
        })
        .map_err(OpError::io)?;
    append_group_event(
        home,
        &updated,
        "group.set_state",
        request,
        json!({"new_state": updated.state}),
    )?;
    object(json!({"group": group_runtime::group(updated)}))
}

fn running(home: &HomeLayout, request: &DaemonRequest, value: bool) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    let runtimes = if value {
        actor_runtime::start_group(home, &group)?
    } else {
        actor_delivery::shutdown_group(&group.group_id);
        actor_runtime::stop_group(&group)?
    };
    let updated = store(home)?
        .mutate(&group.group_id, |doc| {
            doc.running = value;
            if value {
                doc.state = GroupState::Active;
            } else {
                doc.state = GroupState::Stopped;
            }
            Ok(doc.clone())
        })
        .map_err(OpError::io)?;
    let kind = if value { "group.start" } else { "group.stop" };
    append_group_event(home, &updated, kind, request, json!({}))?;
    object(json!({"group": group_runtime::group(updated), "running": value, "runtimes": runtimes}))
}

fn load(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
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

pub(super) fn append_group_event(
    home: &HomeLayout,
    group: &GroupDoc,
    kind: &str,
    request: &DaemonRequest,
    data: Value,
) -> Result<(), OpError> {
    let mut event = Event::new(kind, &group.group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(
        &store(home)?
            .ledger_path(&group.group_id)
            .map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)
}
