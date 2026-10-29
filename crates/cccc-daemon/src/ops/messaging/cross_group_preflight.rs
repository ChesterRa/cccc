use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

/// Mirrors send_cross_group's checks so uploads are refused before any file
/// is stored. Attachment bytes are verified again when the message is sent.
pub(super) fn upload(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = super::load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    cccc_core::permissions::require_group_member(&source, &by)
        .map_err(|error| OpError::new("permission_denied", error.to_string()))?;
    let destination_id = required_arg(request, "dst_group_id")?;
    if let Some(event) = crate::ops::message_idempotency::find(
        home,
        &source.group_id,
        "chat.message",
        &by,
        &request.args,
    ) {
        if event.data.get("dst_group_id").and_then(Value::as_str) != Some(&destination_id) {
            return Err(OpError::new(
                "idempotency_conflict",
                "client_id was already accepted for a different destination",
            ));
        }
        // A delivered message replays its result. One interrupted after the
        // source entry is ready, so the commit completes the destination copy.
        return match crate::ops::message_idempotency::find_relay(home, &destination_id, &event.id) {
            Some(delivered) => object(json!({
                "ready":false,
                "duplicate":true,
                "result":{
                    "source_event":event,"event":delivered,"src_event":event,
                    "dst_event":delivered,"transport":"local","duplicate":true
                }
            })),
            None => object(json!({"ready":true})),
        };
    }
    let destination = store(home)?
        .load(&destination_id)
        .map_err(OpError::not_found)?;
    let mut data = super::upload_preflight_data(request);
    data.remove("dst_group_id");
    super::message_validation::normalize_scope(&destination, &mut data)?;
    crate::ops::messaging_recipients::apply_cross_group_recipient(&destination, &mut data)?;
    crate::ops::messaging_recipients::normalize_chat_preflight(
        &destination,
        &format!("{}::{}", source.group_id, by),
        &mut data,
        false,
    )?;
    super::validate_upload_content(request, &data)?;
    object(json!({"ready":true}))
}

/// `send_files` to another Group: admit the scope files, check the route before
/// storing anything, then send through the one cross-group path.
pub(super) fn send_files(
    home: &HomeLayout,
    request: &DaemonRequest,
    group: &GroupDoc,
    paths: &[Value],
) -> OpResult {
    let mut forwarded = request.clone();
    forwarded.args.remove("paths");
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    if crate::ops::message_idempotency::find(
        home,
        &group.group_id,
        "chat.message",
        &by,
        &request.args,
    )
    .is_some()
    {
        // An accepted send replays, or completes a partial relay, from its
        // record; the scope files are not read again.
        return super::send_cross_group(home, &forwarded);
    }
    let mut check = forwarded.clone();
    // File paths belong to the source scope, not the receiving message scope.
    check.args.remove("path");
    check.args.insert("has_attachments".into(), json!(true));
    upload(home, &check)?;
    let files = super::files::read(group, &by, paths, super::cross_group_attachments::MAX_BYTES)?;
    files.apply(home, group, &mut forwarded.args)?;
    // The source scope root means nothing in the destination Group.
    forwarded.args.remove("path");
    super::send_cross_group(home, &forwarded)
}

/// Preflight for a client_id already accepted as a local cross-group reply,
/// matching the reply op: a changed parent conflicts, a delivered reply replays
/// its result, and one missing its far copy is left for the commit to complete.
pub(super) fn accepted_reply(home: &HomeLayout, event: &Event, reply_to: &str) -> Option<OpResult> {
    if event.data.contains_key("connect_message") {
        return None;
    }
    let destination = event.data.get("dst_group_id").and_then(Value::as_str)?;
    if event.data.get("reply_to").and_then(Value::as_str) != Some(reply_to) {
        return Some(Err(OpError::new(
            "idempotency_conflict",
            "client_id was already accepted for a different reply target",
        )));
    }
    Some(
        match crate::ops::message_idempotency::find_relay(home, destination, &event.id) {
            Some(delivered) => object(json!({
                "ready":false,
                "duplicate":true,
                "result":{
                    "event":event,"message_mode":event.data.get("dst_message_mode"),
                    "source_event":event,"src_event":event,"dst_event":delivered,
                    "transport":"local","duplicate":true
                }
            })),
            None => object(json!({"ready":true})),
        },
    )
}
