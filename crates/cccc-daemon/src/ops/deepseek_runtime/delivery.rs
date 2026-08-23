use super::delivery_projection::{TurnProjection, agent_message_text, persist_terminal};
use super::turn_failure::fail_sent_request;
use super::turn_timeout::settle_timed_out_request;
use super::{cancellation_requested, sessions};
use cccc_contracts::{Actor, Event};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Deliver one DeepSeek ACP prompt and persist provider output before the
/// caller records a cursor completion. A failure returns false so the source
/// event remains unread; permanent deployment/context failures also block
/// automatic restart until the actor is explicitly started again.
pub fn deliver(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
    cancelled: &AtomicBool,
) -> bool {
    deliver_with_timeout(
        home,
        group,
        actor,
        event,
        cancelled,
        Duration::from_secs(cccc_contracts::DEEPSEEK_TURN_TIMEOUT_SECONDS),
    )
}

pub(super) fn deliver_with_timeout(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
    cancelled: &AtomicBool,
    turn_timeout: Duration,
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
    if super::recovery::has_completed_event(home, group, actor, &event.id) {
        return true;
    }
    let payload = crate::ops::actor_delivery_render::render_batch_with_mail_context(
        home,
        group,
        &actor.id,
        std::slice::from_ref(event),
    );
    let Some(mut payload) = payload else {
        return false;
    };
    if event.kind == "chat.message" {
        payload.push_str("\n\n[cccc] ");
        payload.push_str(cccc_core::system_prompt::MESSAGE_DELIVERY_GUIDANCE);
    }
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
    let turn_deadline = Instant::now() + turn_timeout;
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
    let attempt_id = format!("{session_id}:{request_id}");
    let turn_id = format!("deepseek:{}:{attempt_id}", event.id);
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
        Some(&format!("deepseek.turn.started:{}:{attempt_id}", event.id)),
    )
    .is_err()
    {
        fail!(false);
    }
    let mut update_ordinal = 0_u64;
    let mut message_text = String::new();
    macro_rules! projection {
        () => {
            TurnProjection {
                home,
                group,
                actor,
                event,
                turn_id: &turn_id,
                stream_id: &stream_id,
                session_id: &session_id,
                request_id,
                attempt_id: &attempt_id,
                message_text: &message_text,
            }
        };
    }
    macro_rules! timeout {
        () => {{
            let projection = projection!();
            return settle_timed_out_request(&holder, &mut supervisor, &projection);
        }};
    }
    loop {
        if cancelled.load(Ordering::Acquire) || cancellation_requested(&group.group_id, &actor.id) {
            fail!(false);
        }
        let remaining = turn_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timeout!();
        }
        let frame = match supervisor.next_frame(remaining.min(Duration::from_millis(200))) {
            Ok(frame) => frame,
            Err(cccc_runtime::deepseek_supervisor::SupervisorError::Timeout) => {
                if cancelled.load(Ordering::Acquire)
                    || cancellation_requested(&group.group_id, &actor.id)
                {
                    fail!(false);
                }
                if Instant::now() >= turn_deadline {
                    timeout!();
                }
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
            let update_key = format!("deepseek.update:{}:{attempt_id}:{ordinal}", event.id);
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
            let projection = projection!();
            return persist_terminal(&holder, &mut supervisor, &projection, &frame);
        }
        // A response for an unknown id is rejected by the strict parser. A
        // notification with an unknown method is ignored only after protocol
        // validation, preserving forward-compatible ACP notifications.
    }
}
