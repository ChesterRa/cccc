use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
use crate::ops::{actor_delivery, messaging_inbox};

mod delegation;
pub(crate) mod install_command;
mod message_validation;
mod message_wake;
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
        "message_upload_preflight" => message_upload_preflight(home, request),
        "reply_request_cancel" => reply_request_cancel(home, request),
        "message_deliver" => message_deliver(home, request),
        "stream_emit" => stream::emit(home, request),
        "relay_user_delegation" => delegation::relay(home, request),
        "system_notify" => send(home, request, "system.notify"),
        "event_append" => append_raw(home, request),
        "ledger_tail" => super::messaging_query::tail(home, request),
        "ledger_search" => super::messaging_query::search(home, request),
        "ledger_window" => super::messaging_query::window(home, request),
        "ledger_statuses" => super::messaging_status::statuses(home, request),
        "message_read_status" => super::messaging_status::read_status(home, request),
        "inbox_peek" => messaging_inbox::peek(home, request),
        "inbox_read" => messaging_inbox::read(home, request),
        "message_history" => messaging_inbox::history(home, request),
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
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &group.group_id, "chat.message", &by, &request.args)
    {
        return duplicate_send(event);
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

    let mut preflight: Map<String, Value> = request
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by" | "paths"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    super::messaging_recipients::normalize_chat_preflight(&group, &by, &mut preflight, false)?;

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
    cccc_core::permissions::require_group_member(&source, &by)
        .map_err(|error| OpError::new("permission_denied", error.to_string()))?;
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
    let destination_message_mode = data
        .get("message_mode")
        .cloned()
        .unwrap_or_else(|| json!("send"));
    data.insert("to".into(), json!(["user"]));
    data.insert("message_mode".into(), json!("send"));
    data.insert("dst_to".into(), destination_recipients);
    data.insert("dst_message_mode".into(), destination_message_mode);
    data.insert("dst_group_id".into(), json!(destination_id));
    let event = append(home, &source.group_id, "chat.message", &by, data)?;
    object(json!({"source_event":event,"transport":"group_bridge_session"}))
}

fn send_cross_group(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = load(home, request)?;
    let destination_id = required_arg(request, "dst_group_id")?;
    let destination = store(home)?
        .load(&destination_id)
        .map_err(OpError::not_found)?;
    let by = string_arg(request, "by")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "user".into());
    cccc_core::permissions::require_group_member(&source, &by)
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
        if let Some(destination_message_mode) = delivery_data.remove("dst_message_mode") {
            delivery_data.insert("message_mode".into(), destination_message_mode);
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
        false,
    )?;

    let source_event = if let Some(existing) = existing_source {
        existing
    } else {
        let mut source_data = delivery_data.clone();
        let destination_recipients = source_data.get("to").cloned().unwrap_or_else(|| json!([]));
        let destination_message_mode = source_data
            .get("message_mode")
            .cloned()
            .unwrap_or_else(|| json!("send"));
        source_data.insert("to".into(), json!(["user"]));
        source_data.insert("message_mode".into(), json!("send"));
        source_data.insert("dst_to".into(), destination_recipients);
        source_data.insert("dst_message_mode".into(), destination_message_mode);
        source_data.insert("dst_group_id".into(), json!(destination.group_id));
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
    send_with_audience_policy(home, request, kind, false)
}

fn send_with_audience_policy(
    home: &HomeLayout,
    request: &DaemonRequest,
    kind: &str,
    allow_sender_only_audience: bool,
) -> OpResult {
    let mut group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &group.group_id, kind, &by, &request.args)
    {
        return duplicate_send(event);
    }
    let mut data: Map<String, Value> = request
        .args
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "group_id" | "by"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if kind == "chat.message" {
        message_validation::normalize(home, &group, &mut data)?;
        super::messaging_recipients::normalize_chat_data(
            &group,
            &by,
            &mut data,
            allow_sender_only_audience,
        )?;
        if data.get("message_mode").and_then(Value::as_str) != Some("mail") {
            group = message_wake::wake_message_targets(home, group, &by, &data)?;
        }
    } else if kind == "system.notify" {
        if data.contains_key("requires_ack") {
            return Err(OpError::new(
                "unsupported_notify_field",
                "system notifications do not support generic acknowledgement",
            ));
        }
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
    if kind != "chat.message"
        || event.data.get("message_mode").and_then(Value::as_str) != Some("mail")
    {
        let _ = actor_delivery::dispatch(home, &group, &event);
    }
    if kind == "chat.message" {
        let message_mode = event
            .data
            .get("message_mode")
            .and_then(Value::as_str)
            .unwrap_or_default();
        object(json!({"event": event, "message_mode": message_mode}))
    } else {
        object(json!({"event": event}))
    }
}

fn duplicate_send(event: Event) -> OpResult {
    let message_mode = event
        .data
        .get("message_mode")
        .and_then(Value::as_str)
        .unwrap_or_default();
    object(json!({"event":event,"message_mode":message_mode,"duplicate":true}))
}

fn message_upload_preflight(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    match required_arg(request, "operation")?.as_str() {
        "send" => preflight_upload_send(home, request),
        "reply" => preflight_upload_reply(home, request),
        "send_cross_group" => super::group_bridge::preflight_upload(home, request),
        _ => Err(OpError::new(
            "invalid_args",
            "operation must be send, reply, or send_cross_group",
        )),
    }
}

fn preflight_upload_send(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &group.group_id, "chat.message", &by, &request.args)
    {
        return preflight_duplicate(event);
    }
    let mut data = upload_preflight_data(request);
    message_validation::normalize(home, &group, &mut data)?;
    super::messaging_recipients::normalize_chat_preflight(&group, &by, &mut data, false)?;
    validate_upload_content(request, &data)?;
    object(json!({"ready":true}))
}

fn preflight_upload_reply(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if ["priority", "reply_required", "requires_ack"]
        .iter()
        .any(|key| request.args.contains_key(*key))
    {
        return Err(OpError::new(
            "unsupported_message_fields",
            "reply accepts message_mode=send or mail; legacy delivery fields are not supported",
        ));
    }
    let message_mode = string_arg(request, "message_mode")
        .map(|mode| mode.trim().to_ascii_lowercase())
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "send".into());
    if !matches!(message_mode.as_str(), "send" | "mail") {
        return Err(OpError::new(
            "invalid_message_mode",
            "reply message_mode must be send or mail",
        ));
    }
    let reply_to = required_arg(request, "reply_to")?;
    let group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if let Some(event) =
        super::message_idempotency::find(home, &group.group_id, "chat.message", &by, &request.args)
    {
        return preflight_duplicate(event);
    }
    let target = find_event(home, &group.group_id, &reply_to)?;
    let remote_reply =
        super::group_bridge::prepare_reply(home, &group, &target, request, &message_mode)?;
    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("message_mode".into(), Value::String(message_mode));
    forwarded
        .args
        .insert("reply_to".into(), Value::String(reply_to));
    super::message_metadata::add_reply_snapshot(&target, &mut forwarded.args);
    if let Some(prepared) = remote_reply.as_ref() {
        prepared.apply_local_metadata(&target, &mut forwarded.args);
    } else if recipient_tokens(&forwarded.args).is_empty() {
        forwarded.args.insert(
            "to".into(),
            json!(default_reply_recipients(&group, &by, &target)),
        );
    }
    let mut data = upload_preflight_data(&forwarded);
    message_validation::normalize(home, &group, &mut data)?;
    super::messaging_recipients::normalize_chat_preflight(
        &group,
        &by,
        &mut data,
        remote_reply.is_some(),
    )?;
    validate_upload_content(request, &data)?;
    object(json!({"ready":true}))
}

fn upload_preflight_data(request: &DaemonRequest) -> Map<String, Value> {
    request
        .args
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "group_id" | "by" | "operation" | "has_attachments"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn validate_upload_content(
    request: &DaemonRequest,
    data: &Map<String, Value>,
) -> Result<(), OpError> {
    let has_text = data
        .get("text")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty());
    let has_attachments = request
        .args
        .get("has_attachments")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if has_text || has_attachments {
        Ok(())
    } else {
        Err(OpError::new(
            "invalid_args",
            "text or attachments is required",
        ))
    }
}

fn preflight_duplicate(event: Event) -> OpResult {
    let result = duplicate_send(event)?;
    object(json!({"ready":false,"duplicate":true,"result":result}))
}

fn slash_skill_dispatch(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let dispatch = slash_skill::prepare(home, request)?;
    let response = send(home, &dispatch.request, "chat.message")?;
    slash_skill::response(&dispatch, &response)
}

fn reply(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if ["priority", "reply_required", "requires_ack"]
        .iter()
        .any(|key| request.args.contains_key(*key))
    {
        return Err(OpError::new(
            "unsupported_message_fields",
            "reply accepts message_mode=send or mail; legacy delivery fields are not supported",
        ));
    }
    let message_mode = string_arg(request, "message_mode")
        .map(|mode| mode.trim().to_ascii_lowercase())
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "send".into());
    if !matches!(message_mode.as_str(), "send" | "mail") {
        return Err(OpError::new(
            "invalid_message_mode",
            "reply message_mode must be send or mail",
        ));
    }
    let reply_to = required_arg(request, "reply_to")?;
    let group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let target = find_event(home, &group.group_id, &reply_to)?;
    let remote_reply =
        super::group_bridge::prepare_reply(home, &group, &target, request, &message_mode)?;
    let mut forwarded = request.clone();
    forwarded
        .args
        .insert("message_mode".into(), Value::String(message_mode));
    forwarded
        .args
        .insert("reply_to".into(), Value::String(reply_to));
    super::message_metadata::add_reply_snapshot(&target, &mut forwarded.args);
    if let Some(prepared) = remote_reply.as_ref() {
        prepared.apply_local_metadata(&target, &mut forwarded.args);
    } else if recipient_tokens(&forwarded.args).is_empty() {
        forwarded.args.insert(
            "to".into(),
            json!(default_reply_recipients(&group, &by, &target)),
        );
    }
    let mut response =
        send_with_audience_policy(home, &forwarded, "chat.message", remote_reply.is_some())?;
    if let Some(prepared) = remote_reply {
        let source_event_id = response
            .get("event")
            .and_then(|event| event.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let remote_result = if source_event_id.is_empty() {
            json!({"error":{
                "code":"group_bridge_reply_failed",
                "message":"local reply event has no id"
            }})
        } else {
            match prepared.relay(home, request, source_event_id) {
                Ok(result) => Value::Object(result),
                Err(error) => json!({"error":{
                    "code":error.code,"message":error.message,"details":error.details
                }}),
            }
        };
        response.insert("group_bridge_reply".into(), remote_result);
    }
    Ok(response)
}

fn reply_request_cancel(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let source_event_id = required_arg(request, "source_event_id")?;
    let source = find_event(home, &group.group_id, &source_event_id)?;
    let source_message_mode = source
        .data
        .get("dst_message_mode")
        .or_else(|| source.data.get("message_mode"))
        .and_then(Value::as_str);
    if source.kind != "chat.message" || source_message_mode != Some("request_reply") {
        return Err(OpError::new(
            "invalid_source_event",
            "source_event_id must identify a request_reply message",
        ));
    }
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if by != "user" && by != source.by {
        return Err(OpError::new(
            "permission_denied",
            "only the source sender or user may cancel a reply request",
        ));
    }
    let path = store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    if let Some(existing) = cccc_core::ledger::read_all(&path)
        .map_err(OpError::io)?
        .into_iter()
        .find(|event| {
            event.kind == "chat.reply_request.cancelled"
                && event.data.get("source_event_id").and_then(Value::as_str)
                    == Some(source_event_id.as_str())
        })
    {
        let propagation =
            super::group_bridge::cancellation::propagate(home, &group.group_id, &source, &existing);
        return object(json!({
            "event":existing,"duplicate":true,"propagation":propagation
        }));
    }
    let event = append(
        home,
        &group.group_id,
        "chat.reply_request.cancelled",
        &by,
        json!({"source_event_id":source_event_id})
            .as_object()
            .cloned()
            .expect("reply cancellation data"),
    )?;
    let propagation =
        super::group_bridge::cancellation::propagate(home, &group.group_id, &source, &event);
    object(json!({"event":event,"propagation":propagation}))
}

fn message_deliver(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let mut group = load(home, request)?;
    let source_event_id = required_arg(request, "source_event_id")?;
    let source = find_event(home, &group.group_id, &source_event_id)?;
    if source.kind != "chat.message" {
        return Err(OpError::new(
            "invalid_source_event",
            "source_event_id must identify a chat.message",
        ));
    }
    if !matches!(
        source.data.get("message_mode").and_then(Value::as_str),
        Some("send" | "request_reply" | "mail")
    ) {
        return Err(OpError::new(
            "legacy_message",
            "historical messages without message_mode cannot be delivered",
        ));
    }
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if by != "user" && by != source.by {
        return Err(OpError::new(
            "permission_denied",
            "only the source sender or user may request delivery",
        ));
    }
    let actor_ids = request
        .args
        .get("actor_ids")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(|| OpError::new("invalid_actor_ids", "actor_ids must be a non-empty array"))?;
    let force_ambiguous = request
        .args
        .get("force_ambiguous")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ledger_path = store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let events = cccc_core::ledger::read_all(&ledger_path).map_err(OpError::io)?;
    let source_position = events
        .iter()
        .position(|event| event.id == source.id)
        .ok_or_else(|| OpError::new("event_not_found", "source event is not in the ledger"))?;
    let generations = cccc_core::inbox::actor_generation_positions(&events);
    let mut requested = Vec::new();
    for value in actor_ids {
        let actor_id = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OpError::new("invalid_actor_ids", "actor_ids must contain strings"))?;
        if requested
            .iter()
            .any(|existing: &cccc_contracts::Actor| existing.id == actor_id)
        {
            continue;
        }
        let actor = group
            .actors
            .iter()
            .find(|actor| actor.id == actor_id)
            .ok_or_else(|| OpError::new("unknown_actor", format!("unknown actor: {actor_id}")))?;
        if !actor.enabled {
            let mut error =
                OpError::new("delivery_blocked", format!("actor is stopped: {actor_id}"));
            error.details.insert("actor_id".into(), json!(actor_id));
            error
                .details
                .insert("reason".into(), json!("actor_disabled"));
            return Err(error);
        }
        if generations
            .get(actor_id)
            .is_some_and(|generation| *generation > source_position)
            || !cccc_core::inbox::is_for_actor(&group, &source, actor_id)
        {
            return Err(OpError::new(
                "event_not_for_actor",
                format!("event is not addressed to actor: {actor_id}"),
            ));
        }
        requested.push(actor.clone());
    }
    let actor_ids = requested
        .iter()
        .map(|actor| actor.id.clone())
        .collect::<Vec<_>>();
    let delivery_claims = requested
        .iter()
        .map(|actor| {
            (
                actor,
                actor_delivery::delivery_transport(home, &group, actor),
            )
        })
        .collect::<Vec<_>>();
    let (claimed, states) = crate::ops::runtime_delivery::claim_deliveries(
        home,
        &group,
        &delivery_claims,
        &source_event_id,
        force_ambiguous,
    )?;
    if !claimed {
        let (actor_id, state) = actor_ids
            .iter()
            .find_map(|actor_id| {
                let state = states.get(actor_id)?;
                (matches!(state.as_str(), "claimed" | "accepted")
                    || (state == "ambiguous" && !force_ambiguous))
                    .then_some((actor_id.as_str(), state.as_str()))
            })
            .unwrap_or((&actor_ids[0], "claimed"));
        let mut error = match state {
            "accepted" => OpError::new(
                "already_delivered",
                format!("message was already accepted for actor: {actor_id}"),
            ),
            "ambiguous" => OpError::new(
                "delivery_ambiguous",
                format!("delivery may already have occurred for actor: {actor_id}"),
            ),
            _ => OpError::new(
                "delivery_in_progress",
                format!("delivery is already in progress for actor: {actor_id}"),
            ),
        };
        error.details.insert("actor_id".into(), json!(actor_id));
        if state == "ambiguous" {
            error
                .details
                .insert("force_ambiguous_required".into(), json!(true));
        }
        return Err(error);
    }
    group = match message_wake::activate_message_targets(home, group, &actor_ids) {
        Ok(group) => group,
        Err(error) => {
            let reason = format!("group resume failed: {}", error.message);
            let mut settlement_error = None;
            for (actor, transport) in &delivery_claims {
                if let Err(settle_error) = crate::ops::runtime_delivery::append_state(
                    home,
                    &source.group_id,
                    &actor.id,
                    &actor.created_at,
                    &source_event_id,
                    transport,
                    crate::ops::runtime_delivery::DeliveryOutcome::Failed(&reason),
                ) {
                    settlement_error.get_or_insert(settle_error);
                }
            }
            return Err(settlement_error.unwrap_or(error));
        }
    };
    actor_delivery::dispatch_preclaimed(home, &group, &source, &requested);
    object(json!({
        "event": source,
        "actor_ids": actor_ids,
        "delivery_state": "claimed",
    }))
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
    let group_id = required_arg(request, "group_id")?;
    store(home)?
        .load(&group_id)
        .map_err(|_| OpError::new("group_not_found", format!("group not found: {group_id}")))
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
