use cccc_contracts::{DaemonRequest, Event, GroupState};
use cccc_core::active;
use cccc_core::ledger;
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{actor_delivery, actor_runtime, group_runtime};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_create" => create(home, request),
        "group_list" | "groups" => list(home),
        "group_show" => show(home, request),
        "group_update" => update(home, request),
        "group_delete" => delete(home, request),
        "group_reset" => reset(home, request),
        "group_set_state" => set_state(home, request),
        "group_start" => running(home, request, true),
        "group_stop" => running(home, request, false),
        _ => return None,
    })
}

fn create(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let title = string_arg(request, "title").unwrap_or_else(|| "working-group".into());
    let topic = string_arg(request, "topic").unwrap_or_default();
    let group = store(home)?.create(&title, &topic).map_err(OpError::io)?;
    append_group_event(
        home,
        &group,
        "group.create",
        request,
        json!({"title": group.title, "topic": group.topic}),
    )?;
    active::set(home, &group.group_id).map_err(OpError::io)?;
    object(json!({"group": group_runtime::group(group)}))
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
    if active::get(home).map_err(OpError::io)?.as_deref() == Some(&group.group_id) {
        active::clear(home).map_err(OpError::io)?;
    }
    object(json!({"group_id": group.group_id, "deleted": deleted}))
}

fn reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let old = load(home, request)?;
    authorize(&old, request)?;
    actor_delivery::shutdown_group(&old.group_id);
    actor_runtime::stop_group(&old)?;
    store(home)?.delete(&old.group_id).map_err(OpError::io)?;
    let created = store(home)?
        .create(&old.title, &old.topic)
        .map_err(OpError::io)?;
    for item in old.scopes {
        cccc_core::group_scope::attach(&store(home)?, &created.group_id, item)
            .map_err(OpError::io)?;
    }
    let group = store(home)?.load(&created.group_id).map_err(OpError::io)?;
    active::set(home, &group.group_id).map_err(OpError::io)?;
    object(json!({"old_group_id": old.group_id, "group": group_runtime::group(group)}))
}

fn set_state(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    let raw = required_arg(request, "state")?;
    let state: GroupState = serde_json::from_value(Value::String(raw)).map_err(OpError::invalid)?;
    if matches!(state, GroupState::Paused | GroupState::Stopped) {
        actor_delivery::shutdown_group(&group.group_id);
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

fn append_group_event(
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
