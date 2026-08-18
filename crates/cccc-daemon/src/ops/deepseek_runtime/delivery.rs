use super::turn_failure::fail_sent_request;
use super::{cancellation_requested, sessions};
use cccc_contracts::{Actor, Event};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn normalize_turn_error(error: Value) -> (Value, bool) {
    let searchable = error.to_string().to_ascii_lowercase();
    if searchable.contains("no api key") || searchable.contains("deepseek_api_key") {
        return (
            json!({
                "code": "credential_unavailable",
                "category": "environment",
                "message": "DeepSeek API credential is not configured"
            }),
            true,
        );
    }
    (error, false)
}

/// Deliver one DeepSeek ACP prompt and persist provider output before the
/// caller records a cursor completion.  A failed append deliberately returns
/// false so the source event remains unread for recovery.
pub fn deliver(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
    cancelled: &AtomicBool,
) -> bool {
    if cancelled.load(Ordering::Acquire) {
        return false;
    }
    let key = (group.group_id.clone(), actor.id.clone());
    let holder = sessions()
        .read()
        .ok()
        .and_then(|map| map.get(&key).cloned());
    let Some(holder) = holder else {
        return false;
    };
    let payload = crate::ops::actor_delivery_render::render_batch(std::slice::from_ref(event));
    let Some(payload) = payload else {
        return false;
    };
    if !holder.running.load(Ordering::Acquire) {
        return false;
    }
    let Ok(mut supervisor) = holder.supervisor.lock() else {
        return false;
    };
    let Some(session_id) = supervisor.session_id().map(str::to_owned) else {
        return false;
    };
    let request_id = match supervisor.enqueue(payload).and_then(|_| {
        supervisor
            .flush_one(&session_id)
            .map(|value| value.unwrap_or_default())
    }) {
        Ok(id) if id > 0 => id,
        _ => return false,
    };
    macro_rules! fail {
        ($terminal_seen:expr) => {
            return fail_sent_request(
                &holder,
                &mut supervisor,
                &session_id,
                request_id,
                $terminal_seen,
            )
        };
    }
    let turn_id = format!("deepseek:{}", event.id);
    let stream_id = format!("{turn_id}:message");
    if crate::ops::local_headless::append_event_with_dedupe(
        home,
        &group.group_id,
        &actor.id,
        "headless.turn.started",
        Map::from_iter([
            ("event_id".into(), json!(event.id)),
            ("turn_id".into(), json!(turn_id)),
            ("session_id".into(), json!(session_id)),
            ("request_id".into(), json!(request_id)),
            ("status".into(), json!("started")),
        ]),
        Some(&format!("deepseek.turn.started:{}", event.id)),
    )
    .is_err()
    {
        fail!(false);
    }
    let mut update_ordinal = 0_u64;
    let mut message_text = String::new();
    loop {
        if cancelled.load(Ordering::Acquire) || cancellation_requested(&group.group_id, &actor.id) {
            fail!(false);
        }
        let frame = match supervisor.next_frame(Duration::from_millis(200)) {
            Ok(frame) => frame,
            Err(cccc_runtime::deepseek_supervisor::SupervisorError::Timeout)
                if !cancelled.load(Ordering::Acquire)
                    && !cancellation_requested(&group.group_id, &actor.id) =>
            {
                continue;
            }
            Err(_) => {
                fail!(false);
            }
        };
        if frame.get("method") == Some(&Value::String("session/update".into())) {
            let Some(params) = frame.get("params") else {
                fail!(false);
            };
            if cccc_runtime::deepseek_acp::validate_session_update(&frame, &session_id).is_err() {
                fail!(false);
            }
            let update = params.get("update").cloned().unwrap_or(Value::Null);
            let ordinal = update_ordinal;
            update_ordinal = update_ordinal.saturating_add(1);
            let update_key = format!("deepseek.update:{}:{}", event.id, ordinal);
            let (kind, data) = if let Some(delta) = agent_message_text(&update) {
                message_text.push_str(delta);
                (
                    "headless.message.delta",
                    Map::from_iter([
                        ("event_id".into(), json!(event.id)),
                        ("turn_id".into(), json!(turn_id)),
                        ("stream_id".into(), json!(stream_id)),
                        ("delta".into(), json!(delta)),
                    ]),
                )
            } else {
                let update_kind = update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .unwrap_or("ACP update");
                (
                    "headless.activity.updated",
                    Map::from_iter([
                        ("event_id".into(), json!(event.id)),
                        ("turn_id".into(), json!(turn_id)),
                        (
                            "activity_id".into(),
                            json!(format!("{turn_id}:update:{ordinal}")),
                        ),
                        ("kind".into(), json!("thinking")),
                        ("status".into(), json!("updated")),
                        ("summary".into(), json!(update_kind)),
                        ("detail".into(), json!(update.to_string())),
                        ("raw_item_type".into(), json!(update_kind)),
                    ]),
                )
            };
            if crate::ops::local_headless::append_event_with_dedupe(
                home,
                &group.group_id,
                &actor.id,
                kind,
                data,
                Some(&update_key),
            )
            .is_err()
            {
                fail!(false);
            }
            continue;
        }
        if frame.get("method") == Some(&Value::String("session/request_permission".into())) {
            let Some(params) = frame.get("params").and_then(Value::as_object) else {
                fail!(false);
            };
            let Ok(permission_id) =
                cccc_runtime::deepseek_acp::permission_request_id(&frame, &session_id)
            else {
                fail!(false);
            };
            let options = params.get("options").cloned().unwrap_or(Value::Null);
            if supervisor
                .respond_permission(permission_id, &options, false)
                .is_err()
            {
                fail!(false);
            }
            if crate::ops::local_headless::append_event(
                home,
                &group.group_id,
                &actor.id,
                "headless.permission.responded",
                Map::from_iter([
                    ("event_id".into(), json!(event.id)),
                    ("turn_id".into(), json!(turn_id)),
                    ("session_id".into(), json!(session_id)),
                ]),
            )
            .is_err()
            {
                fail!(false);
            }
            continue;
        }
        if frame.get("id") == Some(&json!(request_id)) {
            let stop_reason = cccc_runtime::deepseek_acp::terminal_stop_reason(&frame);
            let cancelled = stop_reason == Some("cancelled");
            let failed = frame.get("error").is_some() || stop_reason != Some("end_turn");
            let kind = if failed {
                "headless.turn.failed"
            } else {
                "headless.turn.completed"
            };
            if !message_text.is_empty()
                && crate::ops::local_headless::append_event_with_dedupe(
                    home,
                    &group.group_id,
                    &actor.id,
                    "headless.message.completed",
                    Map::from_iter([
                        ("event_id".into(), json!(event.id)),
                        ("turn_id".into(), json!(turn_id)),
                        ("stream_id".into(), json!(stream_id)),
                        ("text".into(), json!(message_text)),
                    ]),
                    Some(&format!("deepseek.message.completed:{}", event.id)),
                )
                .is_err()
            {
                fail!(true);
            }
            let (error, credential_failure) = if cancelled {
                (
                    json!({"message":"DeepSeek ACP turn was cancelled","code":"cancelled"}),
                    false,
                )
            } else {
                normalize_turn_error(frame.get("error").cloned().unwrap_or(Value::Null))
            };
            let data = Map::from_iter([
                ("event_id".into(), json!(event.id)),
                ("turn_id".into(), json!(turn_id)),
                ("session_id".into(), json!(session_id)),
                ("request_id".into(), json!(request_id)),
                (
                    "result".into(),
                    frame.get("result").cloned().unwrap_or(Value::Null),
                ),
                ("error".into(), error),
                (
                    "status".into(),
                    json!(if failed { "failed" } else { "completed" }),
                ),
            ]);
            if crate::ops::local_headless::append_event_with_dedupe(
                home,
                &group.group_id,
                &actor.id,
                kind,
                data,
                Some(&format!("deepseek.turn:{}:{}", kind, event.id)),
            )
            .is_err()
            {
                fail!(true);
            }
            if credential_failure {
                holder.running.store(false, Ordering::Release);
                let _ = supervisor.stop();
            }
            return !failed;
        }
        // A response for an unknown id is rejected by the strict parser. A
        // notification with an unknown method is ignored only after protocol
        // validation, preserving forward-compatible ACP notifications.
    }
}

fn agent_message_text(update: &Value) -> Option<&str> {
    (update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk"))
        .then(|| update.pointer("/content/text").and_then(Value::as_str))
        .flatten()
        .filter(|text| !text.is_empty())
}
