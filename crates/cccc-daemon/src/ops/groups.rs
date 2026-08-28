use cccc_contracts::{ActorRole, DaemonRequest, Event, GroupState};
use cccc_core::active;
use cccc_core::actors;
use cccc_core::group_prompts::{
    BUILTIN_HELP_MARKDOWN, DEFAULT_PREAMBLE_BODY, HELP_FILENAME, PREAMBLE_FILENAME,
    compose_effective_help_markdown, delete_help, delete_preamble, parse_help_markdown, read_help,
    read_preamble, select_help_markdown, update_actor_help_note, write_help, write_preamble,
};
use cccc_core::ledger;
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout, group_bridge_legacy};
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
        "group_help_get" => help_get(home, request),
        "actor_notes_get" => actor_notes_get(home, request),
        "actor_notes_set" => actor_notes_write(home, request, false),
        "actor_notes_clear" => actor_notes_write(home, request, true),
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
    let state = group_bridge_legacy::load(home).map_err(OpError::io)?;
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

struct HelpSource {
    content: String,
    effective_content: String,
    source: &'static str,
    path: String,
    source_path: String,
    overridden: bool,
}

fn help_source(home: &HomeLayout, group_id: &str) -> Result<HelpSource, OpError> {
    let prompt = read_help(&store(home)?, group_id).map_err(OpError::io)?;
    let path = prompt.path.to_string_lossy().into_owned();
    let override_content = prompt.content.unwrap_or_default();
    let overridden = prompt.found && !override_content.trim().is_empty();
    let content = if overridden {
        override_content
    } else {
        BUILTIN_HELP_MARKDOWN.to_owned()
    };
    let effective_content = if overridden {
        compose_effective_help_markdown(BUILTIN_HELP_MARKDOWN, &content)
    } else {
        content.clone()
    };
    Ok(HelpSource {
        content,
        effective_content,
        source: if overridden { "home" } else { "builtin" },
        source_path: if overridden {
            path.clone()
        } else {
            "resources/cccc-help.md".into()
        },
        path,
        overridden,
    })
}

fn help_group_id(request: &DaemonRequest) -> Result<String, OpError> {
    string_arg(request, "group_id")
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_owned())
        .ok_or_else(|| OpError::new("missing_group_id", "missing group_id"))
}

fn load_help_group(home: &HomeLayout, group_id: &str) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(group_id)
        .map_err(|_| OpError::new("group_not_found", format!("group not found: {group_id}")))
}

fn canonical_actor_id(group: &GroupDoc, value: &str, extras: &[String]) -> String {
    let value = value.trim();
    group
        .actors
        .iter()
        .map(|actor| actor.id.as_str())
        .chain(extras.iter().map(String::as_str))
        .find(|candidate| candidate.eq_ignore_ascii_case(value))
        .unwrap_or(value)
        .to_owned()
}

fn caller_role(group: &GroupDoc, by: &str) -> Result<Option<ActorRole>, OpError> {
    if matches!(by, "user" | "system") {
        return Ok(None);
    }
    actors::effective_role(group, by).map(Some).ok_or_else(|| {
        OpError::new(
            "permission_denied",
            format!("group help requires a known actor: {by}"),
        )
    })
}

fn help_get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = help_group_id(request)?;
    let group = load_help_group(home, &group_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let role = caller_role(&group, &by)?;
    let requested_actor_id = string_arg(request, "actor_id").unwrap_or_default();
    let actor_id = if requested_actor_id.trim().is_empty() && role.is_some() {
        by.clone()
    } else {
        requested_actor_id
    };
    let actor = if actor_id.trim().is_empty() {
        None
    } else {
        Some(
            group
                .actors
                .iter()
                .find(|actor| actor.id.eq_ignore_ascii_case(actor_id.trim()))
                .ok_or_else(|| {
                    OpError::new("actor_not_found", format!("actor not found: {actor_id}"))
                })?,
        )
    };
    if role == Some(ActorRole::Peer)
        && actor.is_some_and(|actor| !actor.id.eq_ignore_ascii_case(&by))
    {
        return Err(OpError::new(
            "permission_denied",
            "actors can only read their own effective help",
        ));
    }
    let canonical_actor_id = actor.map(|actor| actor.id.as_str()).unwrap_or_default();
    let voice_secretary = actor.is_some_and(|actor| {
        actor.internal_kind.as_deref().map(str::trim) == Some("voice_secretary")
    });
    let selected_role = if voice_secretary {
        Some("voice_secretary")
    } else {
        actor
            .and_then(|actor| actors::effective_role(&group, &actor.id))
            .map(|role| match role {
                ActorRole::Foreman => "foreman",
                ActorRole::Peer => "peer",
            })
    };
    let help = help_source(home, &group_id)?;
    object(json!({
        "group_id": group_id,
        "actor_id": if canonical_actor_id.is_empty() { Value::Null } else { json!(canonical_actor_id) },
        "source": help.source,
        "source_path": help.source_path,
        "filename": HELP_FILENAME,
        "overridden": help.overridden,
        "markdown": select_help_markdown(
            &help.effective_content,
            selected_role,
            (!canonical_actor_id.is_empty()).then_some(canonical_actor_id),
            voice_secretary,
        ),
    }))
}

fn authorize_actor_notes(
    group: &GroupDoc,
    by: &str,
    target_actor_id: &str,
    mutate: bool,
) -> Result<(), OpError> {
    let role = caller_role(group, by)?;
    if mutate && role == Some(ActorRole::Peer) {
        return Err(OpError::new(
            "permission_denied",
            "modifying actor notes requires foreman or user access",
        ));
    }
    if !mutate && role == Some(ActorRole::Peer) {
        if target_actor_id.trim().is_empty() {
            return Err(OpError::new(
                "permission_denied",
                "listing all actor notes requires foreman or user access",
            ));
        }
        if !target_actor_id.eq_ignore_ascii_case(by) {
            return Err(OpError::new(
                "permission_denied",
                "actors can only read their own actor notes",
            ));
        }
    }
    Ok(())
}

fn actor_notes_get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = help_group_id(request)?;
    let group = load_help_group(home, &group_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let requested = string_arg(request, "target_actor_id").unwrap_or_default();
    let help = help_source(home, &group_id)?;
    let parsed = parse_help_markdown(&help.content);
    let actor_ids = group
        .actors
        .iter()
        .map(|actor| actor.id.clone())
        .collect::<Vec<_>>();
    let extras = parsed
        .actor_notes
        .keys()
        .filter(|actor_id| !actor_ids.contains(actor_id))
        .cloned()
        .collect::<Vec<_>>();
    let target = canonical_actor_id(&group, &requested, &extras);
    authorize_actor_notes(&group, &by, &target, false)?;
    if !requested.trim().is_empty() {
        return object(json!({
            "target_actor_id": target,
            "content": parsed.actor_notes.get(&target).cloned().unwrap_or_default(),
            "source": help.source,
            "path": help.path,
        }));
    }
    let mut ordered = actor_ids;
    for actor_id in extras {
        if !ordered.contains(&actor_id) {
            ordered.push(actor_id);
        }
    }
    object(json!({
        "actor_notes": ordered.into_iter().map(|actor_id| json!({
            "content": parsed.actor_notes.get(&actor_id).cloned().unwrap_or_default(),
            "actor_id": actor_id,
        })).collect::<Vec<_>>(),
        "source": help.source,
        "path": help.path,
    }))
}

fn actor_notes_write(home: &HomeLayout, request: &DaemonRequest, clear: bool) -> OpResult {
    let group_id = help_group_id(request)?;
    let requested = required_arg(request, "target_actor_id")?;
    let content = if clear {
        String::new()
    } else {
        request
            .args
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| OpError::new("invalid_args", "content must be a string"))?
    };
    let group = load_help_group(home, &group_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    authorize_actor_notes(&group, &by, &requested, true)?;
    let target = canonical_actor_id(&group, &requested, &[]);
    if !group.actors.iter().any(|actor| actor.id == target) {
        return Err(OpError::new(
            "actor_not_found",
            format!("actor not found: {requested}"),
        ));
    }
    let help = help_source(home, &group_id)?;
    let actor_order = group
        .actors
        .iter()
        .map(|actor| actor.id.clone())
        .collect::<Vec<_>>();
    let next = update_actor_help_note(&help.content, &target, &content, &actor_order);
    let changed = next != help.content;
    if changed {
        let group_store = store(home)?;
        if next.trim().is_empty()
            || next == BUILTIN_HELP_MARKDOWN
            || parse_help_markdown(&next) == parse_help_markdown(BUILTIN_HELP_MARKDOWN)
        {
            delete_help(&group_store, &group_id).map_err(OpError::io)?;
        } else {
            write_help(&group_store, &group_id, &next).map_err(OpError::io)?;
        }
    }
    let refreshed = help_source(home, &group_id)?;
    let parsed = parse_help_markdown(&refreshed.content);
    object(json!({
        "target_actor_id": target,
        "content": parsed.actor_notes.get(&target).cloned().unwrap_or_default(),
        "source": refreshed.source,
        "path": refreshed.path,
        "changed": changed,
    }))
}

fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let current = load(home, request)?;
    authorize(&current, request)?;
    let patch = group_update_patch(request)?;
    let title = patch
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty());
    let topic = patch
        .get("topic")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let group = store(home)?
        .update(&current.group_id, title.as_deref(), topic.as_deref())
        .map_err(OpError::not_found)?;
    let event = append_group_event(
        home,
        &group,
        "group.update",
        request,
        json!({"patch": patch}),
    )?;
    let group_id = group.group_id.clone();
    object(json!({"group_id": group_id, "group": group_runtime::group(group), "event": event}))
}

fn group_update_patch(request: &DaemonRequest) -> Result<serde_json::Map<String, Value>, OpError> {
    let patch = match request.args.get("patch") {
        Some(Value::Object(patch)) => patch.clone(),
        Some(_) => {
            return Err(OpError::new("invalid_patch", "patch must be an object"));
        }
        None => {
            let mut patch = serde_json::Map::new();
            for key in ["title", "topic"] {
                if let Some(value) = request.args.get(key)
                    && !value.is_null()
                {
                    patch.insert(key.into(), value.clone());
                }
            }
            patch
        }
    };
    let mut unknown = patch
        .keys()
        .filter(|key| !matches!(key.as_str(), "title" | "topic"))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        unknown.sort();
        let mut error = OpError::new("invalid_patch", "invalid patch keys");
        error.details.insert("unknown_keys".into(), json!(unknown));
        return Err(error);
    }
    if patch.is_empty() {
        return Err(OpError::new("invalid_patch", "empty patch"));
    }
    if let Some((key, _)) = patch.iter().find(|(_, value)| !value.is_string()) {
        return Err(OpError::new(
            "invalid_patch",
            format!("{key} must be a string"),
        ));
    }
    Ok(patch)
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
    let resumes_automation = matches!(group.state, GroupState::Paused)
        && matches!(state, GroupState::Active | GroupState::Idle)
        || matches!(group.state, GroupState::Idle) && matches!(state, GroupState::Active);
    if resumes_automation {
        cccc_core::automation::reset_rule_timers_on_resume(home, &group.group_id)
            .map_err(OpError::io)?;
    }
    if matches!(state, GroupState::Paused | GroupState::Stopped) {
        actor_delivery::shutdown_group(&group.group_id);
        super::local_headless::stop_group(&group.group_id);
        super::deepseek_runtime::stop_group(&group.group_id);
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
    if matches!(updated.state, GroupState::Active | GroupState::Idle) {
        actor_delivery::dispatch_group_unread(home, &updated);
    }
    object(json!({"group": group_runtime::group(updated)}))
}

fn running(home: &HomeLayout, request: &DaemonRequest, value: bool) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    if value && !group.running {
        cccc_core::automation::reset_rule_timers_on_resume(home, &group.group_id)
            .map_err(OpError::io)?;
    }
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
    if value {
        actor_delivery::dispatch_group_unread(home, &updated);
    }
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
) -> Result<Event, OpError> {
    let mut event = Event::new(kind, &group.group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(
        &store(home)?
            .ledger_path(&group.group_id)
            .map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    Ok(event)
}
