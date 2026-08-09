use cccc_contracts::{DaemonRequest, Event, GroupState};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{actor_delivery, messaging_inbox};

mod delegation;
pub(crate) mod install_command;
mod message_validation;
mod slash_skill;
mod stream;
mod tracked_send;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "send" | "message_send" => send(home, request, "chat.message"),
        "send_files" => send_files(home, request),
        "send_cross_group" => send_cross_group(home, request),
        "send_cross_group_remote_record" => send_cross_group_remote_record(home, request),
        "tracked_send" => tracked_send::handle(home, request),
        "slash_skill_dispatch" => slash_skill_dispatch(home, request),
        "reply" => reply(home, request),
        "stream_emit" => stream::emit(home, request),
        "relay_user_delegation" => delegation::relay(home, request),
        "system_notify" => send(home, request, "system.notify"),
        "event_append" => append_raw(home, request),
        "ledger_tail" => super::messaging_query::tail(home, request),
        "ledger_search" => super::messaging_query::search(home, request),
        "ledger_window" => super::messaging_query::window(home, request),
        "ledger_statuses" => super::messaging_status::statuses(home, request),
        "message_read_status" => super::messaging_status::read_status(home, request),
        "inbox_list" => messaging_inbox::list(home, request),
        "inbox_mark_read" => messaging_inbox::mark_read(home, request),
        "inbox_mark_all_read" => messaging_inbox::mark_all(home, request),
        "chat_ack" => messaging_inbox::ack(home, request, "chat.ack"),
        "notify_ack" => messaging_inbox::ack(home, request, "system.notify_ack"),
        _ => return None,
    })
}

fn send_files(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let paths = request
        .args
        .get("paths")
        .and_then(Value::as_array)
        .filter(|paths| !paths.is_empty())
        .ok_or_else(|| OpError::new("invalid_paths", "paths must be a non-empty array"))?;
    if request
        .args
        .get("attachments")
        .is_some_and(|attachments| !attachments.is_null())
    {
        return Err(OpError::new(
            "invalid_attachments",
            "send_files owns attachments; do not provide attachment records",
        ));
    }

    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key && !scope.url.trim().is_empty())
        .ok_or_else(|| OpError::new("missing_scope", "group has no active scope"))?;
    let root = fs::canonicalize(Path::new(&scope.url))
        .map_err(|error| OpError::new("missing_scope", error.to_string()))?;

    let mut sources: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(paths.len());
    for raw_path in paths {
        let raw = raw_path
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OpError::new("invalid_path", "file path must be a non-empty string"))?;
        let candidate = Path::new(raw);
        let candidate = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            root.join(candidate)
        };
        let source = fs::canonicalize(&candidate).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                OpError::new(
                    "not_found",
                    format!("file not found: {}", candidate.display()),
                )
            } else {
                OpError::new("read_failed", error.to_string())
            }
        })?;
        if !source.starts_with(&root) {
            return Err(OpError::new(
                "invalid_path",
                "file path must be under the group's active scope root",
            ));
        }
        if !source.is_file() {
            return Err(OpError::new(
                "not_found",
                format!("file not found: {}", source.display()),
            ));
        }
        let data =
            fs::read(&source).map_err(|error| OpError::new("read_failed", error.to_string()))?;
        sources.push((source, data));
    }

    let mut attachments = Vec::with_capacity(sources.len());
    let mut titles = Vec::with_capacity(sources.len());
    for (source, data) in sources {
        let title = source
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("file")
            .to_owned();
        let mime_type = mime_guess::from_path(&source)
            .first_or_octet_stream()
            .essence_str()
            .to_owned();
        let kind = if mime_type.starts_with("image/") {
            "image"
        } else {
            "file"
        };
        let blob = cccc_core::blobs::store(home, &group.group_id, &data).map_err(OpError::io)?;
        attachments.push(json!({
            "kind":kind,
            "path":blob.path,
            "title":title,
            "mime_type":mime_type,
            "bytes":blob.bytes,
            "sha256":blob.sha256,
        }));
        titles.push(title);
    }

    let mut forwarded = request.clone();
    forwarded.args.remove("paths");
    forwarded
        .args
        .insert("attachments".into(), Value::Array(attachments));
    forwarded.args.insert(
        "path".into(),
        Value::String(root.to_string_lossy().into_owned()),
    );
    if string_arg(&forwarded, "text").is_none_or(|text| text.trim().is_empty()) {
        forwarded.args.insert(
            "text".into(),
            Value::String(format!("[files] {}", titles.join(", "))),
        );
    }
    send(home, &forwarded, "chat.message")
}

fn send_cross_group_remote_record(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = load(home, request)?;
    let destination_id = required_arg(request, "dst_group_id")?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &source.group_id, "chat.message", &by, &request.args)
    {
        return object(
            json!({"source_event":event,"transport":"group_bridge_session","duplicate":true}),
        );
    }
    let text = string_arg(request, "text").unwrap_or_default();
    let attachments = request
        .args
        .get("attachments")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if text.trim().is_empty()
        && attachments
            .as_array()
            .is_none_or(|attachments| attachments.is_empty())
    {
        return Err(OpError::new(
            "invalid_args",
            "text or attachments is required",
        ));
    }
    let mut data: Map<String, Value> = request
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by" | "dst_group_id"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    super::messaging_recipients::normalize_remote_chat_data(&mut data)?;
    let destination_recipients = data
        .get("to")
        .cloned()
        .unwrap_or_else(|| json!([cccc_core::actors::CROSS_GROUP_FOREMAN_RECIPIENT]));
    data.insert("to".into(), json!(["user"]));
    data.insert("dst_to".into(), destination_recipients);
    data.insert("dst_group_id".into(), json!(destination_id));
    data.insert("transport".into(), json!("group_bridge_session"));
    let event = append(home, &source.group_id, "chat.message", &by, data)?;
    object(json!({"source_event":event,"transport":"group_bridge_session"}))
}

fn send_cross_group(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = load(home, request)?;
    let destination_id = required_arg(request, "dst_group_id")?;
    let destination = store(home)?
        .load(&destination_id)
        .map_err(OpError::not_found)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    cccc_core::permissions::require_group(&source, &by)
        .map_err(|error| OpError::new("permission_denied", error.to_string()))?;
    let text = string_arg(request, "text").unwrap_or_default();
    let attachments = request
        .args
        .get("attachments")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if text.trim().is_empty()
        && attachments
            .as_array()
            .is_none_or(|attachments| attachments.is_empty())
    {
        return Err(OpError::new(
            "invalid_args",
            "text or attachments is required",
        ));
    }
    let existing_source = super::message_idempotency::find(
        home,
        &source.group_id,
        "chat.message",
        &by,
        &request.args,
    );
    if let Some(source_event) = existing_source.as_ref()
        && let Some(event) =
            super::message_idempotency::find_relay(home, &destination.group_id, &source_event.id)
    {
        return object(json!({
            "source_event":source_event,
            "event":event,
            "src_event":source_event,
            "dst_event":event,
            "transport":"local",
            "duplicate":true
        }));
    }

    let destination_by = format!("{}::{}", source.group_id, by);
    let mut delivery_data: Map<String, Value> = existing_source.as_ref().map_or_else(
        || {
            request
                .args
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by" | "dst_group_id"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        },
        |event| event.data.clone(),
    );
    if existing_source.is_none() {
        super::message_metadata::add_sender_snapshot(&source, &by, &mut delivery_data);
    }
    if existing_source.is_some() {
        // The accepted source event is authoritative on a relay retry.
        delivery_data.remove("require_peer_insight");
        if let Some(destination_recipients) = delivery_data.remove("dst_to") {
            delivery_data.insert("to".into(), destination_recipients);
        }
    }
    delivery_data.remove("transport");
    delivery_data.remove("dst_group_id");
    delivery_data.remove("to_group_id");
    super::messaging_recipients::apply_cross_group_recipient(&destination, &mut delivery_data)?;
    super::messaging_recipients::normalize_chat_data(
        &destination,
        &destination_by,
        &mut delivery_data,
    )?;

    let source_event = if let Some(existing) = existing_source {
        existing
    } else {
        let mut source_data = delivery_data.clone();
        let destination_recipients = source_data.get("to").cloned().unwrap_or_else(|| json!([]));
        source_data.insert("to".into(), json!(["user"]));
        source_data.insert("dst_to".into(), destination_recipients);
        source_data.insert("dst_group_id".into(), json!(destination.group_id));
        source_data.insert("transport".into(), json!("local"));
        append(home, &source.group_id, "chat.message", &by, source_data)?
    };

    let mut forwarded = request.clone();
    forwarded.args = delivery_data;
    forwarded
        .args
        .insert("group_id".into(), json!(destination.group_id));
    forwarded.args.insert("by".into(), json!(destination_by));
    forwarded
        .args
        .insert("src_group_id".into(), json!(source.group_id));
    forwarded
        .args
        .insert("src_event_id".into(), json!(source_event.id));
    let destination_response = send(home, &forwarded, "chat.message")?;
    object(json!({
        "source_event":source_event,
        "event":destination_response.get("event"),
        "src_event":source_event,
        "dst_event":destination_response.get("event"),
        "transport":"local"
    }))
}

pub(super) fn send(home: &HomeLayout, request: &DaemonRequest, kind: &str) -> OpResult {
    let mut group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &group.group_id, kind, &by, &request.args)
    {
        return object(json!({
            "event":event,
            "delivery":{"accepted":true,"state":"duplicate","targeted":0,"online":0,"queued":0},
            "duplicate":true
        }));
    }
    let mut data: Map<String, Value> = request
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if kind == "chat.message" {
        message_validation::normalize(home, &group, &mut data)?;
        super::messaging_recipients::normalize_chat_data(&group, &by, &mut data)?;
        group = wake_idle_group(home, group, &by)?;
    } else if kind == "system.notify" {
        match data.get("im_visibility") {
            None | Some(Value::Null) => {
                data.insert("im_visibility".into(), json!("internal"));
            }
            Some(Value::String(value)) if matches!(value.as_str(), "internal" | "public") => {}
            Some(_) => {
                return Err(OpError::new(
                    "invalid_im_visibility",
                    "im_visibility must be internal or public",
                ));
            }
        }
    }
    let event = append(home, &group.group_id, kind, &by, data)?;
    let delivery = actor_delivery::dispatch(home, &group, &event);
    object(json!({"event": event, "delivery": delivery}))
}

fn slash_skill_dispatch(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let dispatch = slash_skill::prepare(home, request)?;
    let response = send(home, &dispatch.request, "chat.message")?;
    slash_skill::response(&dispatch, &response)
}

fn reply(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let reply_to = required_arg(request, "reply_to")?;
    let group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let target = find_event(home, &group.group_id, &reply_to)?;
    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("reply_to".into(), Value::String(reply_to));
    super::message_metadata::add_reply_snapshot(&target, &mut forwarded.args);
    if recipient_tokens(&forwarded.args).is_empty() {
        forwarded.args.insert(
            "to".into(),
            json!(default_reply_recipients(&group, &by, &target)),
        );
    }
    let response = send(home, &forwarded, "chat.message")?;
    // A reply is already durable at this point; acknowledgement is a
    // best-effort follow-up and must not turn a successful reply into failure.
    let ack_event = reply_ack(home, &group, &target, &by).unwrap_or(None);
    let mut response = response;
    response.insert(
        "ack_event".into(),
        ack_event.map_or(Value::Null, |event| {
            serde_json::to_value(event).unwrap_or(Value::Null)
        }),
    );
    Ok(response)
}

fn wake_idle_group(home: &HomeLayout, group: GroupDoc, by: &str) -> Result<GroupDoc, OpError> {
    if group.state != GroupState::Idle
        || by.is_empty()
        || by == "system"
        || group.actors.iter().any(|actor| actor.id == by)
    {
        return Ok(group);
    }
    let store = store(home)?;
    store
        .mutate(&group.group_id, |current| {
            if current.state == GroupState::Idle {
                current.state = GroupState::Active;
            }
            Ok(current.clone())
        })
        .map_err(OpError::io)
}

fn reply_ack(
    home: &HomeLayout,
    group: &GroupDoc,
    target: &Event,
    by: &str,
) -> Result<Option<Event>, OpError> {
    let requires_ack = target.kind == "chat.message"
        && by != target.by
        && target.data.get("priority").and_then(Value::as_str) == Some("attention")
        && cccc_core::inbox::is_for_actor(group, target, by);
    if !requires_ack {
        return Ok(None);
    }
    let ledger_path = store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let exists = cccc_core::ledger::read_all(&ledger_path)
        .map_err(OpError::io)?
        .iter()
        .any(|event| {
            event.kind == "chat.ack"
                && event.data.get("event_id").and_then(Value::as_str) == Some(target.id.as_str())
                && event.data.get("actor_id").and_then(Value::as_str) == Some(by)
        });
    if exists {
        return Ok(None);
    }
    append(
        home,
        &group.group_id,
        "chat.ack",
        by,
        json!({"actor_id":by,"event_id":target.id})
            .as_object()
            .cloned()
            .unwrap_or_default(),
    )
    .map(Some)
}

fn recipient_tokens(args: &Map<String, Value>) -> Vec<String> {
    args.get("to")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn default_reply_recipients(group: &GroupDoc, by: &str, target: &Event) -> Vec<String> {
    let original_by = target.by.trim();
    if !original_by.is_empty() && original_by != by {
        return vec![if original_by == "@user" {
            "user".into()
        } else {
            original_by.into()
        }];
    }
    let original_to = recipient_tokens(&target.data);
    if !original_to.is_empty() {
        return original_to;
    }
    vec![super::messaging_recipients::default_recipient(group).into()]
}

fn append_raw(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let raw = request
        .args
        .get("event")
        .ok_or_else(|| OpError::new("invalid_args", "event is required"))?;
    let event: Event = serde_json::from_value(raw.clone()).map_err(OpError::invalid)?;
    let path = store(home)?
        .ledger_path(&event.group_id)
        .map_err(OpError::io)?;
    if !path.exists() {
        return Err(OpError::new("group_not_found", "group not found"));
    }
    cccc_core::ledger::append(&path, &event).map_err(OpError::io)?;
    object(json!({"event": event}))
}

pub(super) fn append(
    home: &HomeLayout,
    group_id: &str,
    kind: &str,
    by: &str,
    data: Map<String, Value>,
) -> Result<Event, OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = by.into();
    event.data = data;
    cccc_core::ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    Ok(event)
}

pub(super) fn load(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)
}

pub(super) fn find_event(
    home: &HomeLayout,
    group_id: &str,
    event_id: &str,
) -> Result<Event, OpError> {
    cccc_core::ledger::find_event(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        event_id,
    )
    .map_err(OpError::io)?
    .ok_or_else(|| OpError::new("event_not_found", format!("event not found: {event_id}")))
}
