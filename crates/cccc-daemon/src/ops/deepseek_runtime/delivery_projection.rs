use super::RuntimeEntry;
use super::delivery::TurnDelivery;
use cccc_contracts::{Actor, Event};
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::deepseek_supervisor::DeepSeekSupervisor;
use serde_json::{Map, Value, json};
use std::sync::atomic::Ordering;

pub(super) struct TurnProjection<'a> {
    pub(super) home: &'a HomeLayout,
    pub(super) group: &'a GroupDoc,
    pub(super) actor: &'a Actor,
    pub(super) event: &'a Event,
    pub(super) turn_id: &'a str,
    pub(super) stream_id: &'a str,
    pub(super) session_id: &'a str,
    pub(super) request_id: u64,
    pub(super) attempt_id: &'a str,
    pub(super) message_text: &'a str,
}

pub(super) fn normalize_turn_error(error: Value, private_values: &[String]) -> (Value, bool) {
    if error.is_null() {
        return (error, false);
    }
    let searchable = error.to_string().to_ascii_lowercase();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let gateway_unauthorized = message.eq_ignore_ascii_case("turn failed: unauthorized")
        || message.eq_ignore_ascii_case("internal error: turn failed: unauthorized");
    if gateway_unauthorized
        || [
            "invalid token",
            "invalid api key",
            "invalid_api_key",
            "incorrect api key",
            "authentication failed",
            "authentication fails",
            "authentication_error",
        ]
        .into_iter()
        .any(|message| searchable.contains(message))
    {
        return (
            json!({
                "code": "credential_rejected",
                "category": "environment",
                "message": "DeepSeek API credential was rejected; update the saved API key and restart the actor"
            }),
            true,
        );
    }
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
    if [
        "context_length_exceeded",
        "context_window_exceeded",
        "context length exceeded",
        "maximum context length",
    ]
    .into_iter()
    .any(|token| searchable.contains(token))
    {
        return (
            json!({
                "code": "context_window_exceeded",
                "category": "context",
                "message": "DeepSeek request exceeded the model context window; restart the actor to create a fresh session"
            }),
            true,
        );
    }
    (redact_private_values(error, private_values), false)
}

fn redact_private_values(value: Value, private_values: &[String]) -> Value {
    fn redact(text: String, private_values: &[String]) -> String {
        private_values
            .iter()
            .filter(|value| !value.is_empty())
            .fold(text, |text, value| text.replace(value, "[redacted]"))
    }

    match value {
        Value::String(text) => Value::String(redact(text, private_values)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_private_values(item, private_values))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    (
                        redact(key, private_values),
                        redact_private_values(value, private_values),
                    )
                })
                .collect(),
        ),
        Value::Number(number) => {
            let text = number.to_string();
            let redacted = redact(text.clone(), private_values);
            if redacted == text {
                Value::Number(number)
            } else {
                Value::String(redacted)
            }
        }
        Value::Bool(boolean) => {
            let text = boolean.to_string();
            let redacted = redact(text.clone(), private_values);
            if redacted == text {
                Value::Bool(boolean)
            } else {
                Value::String(redacted)
            }
        }
        other => other,
    }
}

pub(super) fn agent_message_text(update: &Value) -> Option<&str> {
    (update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk"))
        .then(|| update.pointer("/content/text").and_then(Value::as_str))
        .flatten()
        .filter(|text| !text.is_empty())
}

pub(super) fn persist_message_completed(projection: &TurnProjection<'_>) -> std::io::Result<()> {
    if projection.message_text.is_empty() {
        return Ok(());
    }
    crate::ops::local_headless::append_event_with_dedupe(
        projection.home,
        &projection.group.group_id,
        &projection.actor.id,
        "headless.message.completed",
        Map::from_iter([
            ("event_id".into(), json!(projection.event.id)),
            ("turn_id".into(), json!(projection.turn_id)),
            ("stream_id".into(), json!(projection.stream_id)),
            ("text".into(), json!(projection.message_text)),
        ]),
        Some(&format!(
            "deepseek.message.completed:{}:{}",
            projection.event.id, projection.attempt_id
        )),
    )
}

pub(super) fn persist_terminal(
    holder: &RuntimeEntry,
    supervisor: &mut DeepSeekSupervisor,
    projection: &TurnProjection<'_>,
    frame: &Value,
) -> TurnDelivery {
    let stop_reason = cccc_runtime::deepseek_acp::terminal_stop_reason(frame);
    let cancelled = stop_reason == Some("cancelled");
    let failed = frame.get("error").is_some() || stop_reason != Some("end_turn");
    let kind = if failed {
        "headless.turn.failed"
    } else {
        "headless.turn.completed"
    };
    if persist_message_completed(projection).is_err() {
        return TurnDelivery::Incomplete;
    }
    let (error, manual_restart_required) = if cancelled {
        (
            json!({"message":"DeepSeek ACP turn was cancelled","code":"cancelled"}),
            false,
        )
    } else {
        normalize_turn_error(
            frame.get("error").cloned().unwrap_or(Value::Null),
            &holder.private_values,
        )
    };
    let result = frame.get("result").cloned().unwrap_or(Value::Null);
    let result = if failed {
        redact_private_values(result, &holder.private_values)
    } else {
        result
    };
    let data = Map::from_iter([
        ("event_id".into(), json!(projection.event.id)),
        ("turn_id".into(), json!(projection.turn_id)),
        ("session_id".into(), json!(projection.session_id)),
        ("request_id".into(), json!(projection.request_id)),
        ("result".into(), result),
        ("error".into(), error),
        (
            "status".into(),
            json!(if failed { "failed" } else { "completed" }),
        ),
    ]);
    let reason_code = data
        .get("error")
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let reason = data
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("DeepSeek ACP turn failed")
        .to_owned();
    let dedupe_key = if failed {
        format!(
            "deepseek.turn:{kind}:{}:{}",
            projection.event.id, projection.attempt_id
        )
    } else {
        format!("deepseek.turn:{kind}:{}", projection.event.id)
    };
    if crate::ops::local_headless::append_event_with_dedupe(
        projection.home,
        &projection.group.group_id,
        &projection.actor.id,
        kind,
        data,
        Some(&dedupe_key),
    )
    .is_err()
    {
        return TurnDelivery::Incomplete;
    }
    if manual_restart_required {
        match cccc_core::deepseek_restart_gate::require_manual_restart(
            projection.home,
            &projection.group.group_id,
            &projection.actor.id,
            &projection.actor.created_at,
            &holder.generation,
            &reason_code,
        ) {
            Ok(true) => {
                holder
                    .manual_restart_required
                    .store(true, Ordering::Release);
                holder.running.store(false, Ordering::Release);
                let _ = supervisor.stop();
            }
            Ok(false) => tracing::warn!(
                group_id = %projection.group.group_id,
                actor_id = %projection.actor.id,
                "ignored a stale DeepSeek permanent failure from a replaced generation"
            ),
            Err(error) => {
                holder
                    .manual_restart_required
                    .store(true, Ordering::Release);
                holder.running.store(false, Ordering::Release);
                let _ = supervisor.stop();
                tracing::error!(
                    %error,
                    group_id = %projection.group.group_id,
                    actor_id = %projection.actor.id,
                    "failed to persist DeepSeek manual restart gate"
                );
            }
        }
    }
    match (failed, cancelled) {
        (false, _) => TurnDelivery::Completed,
        (true, true) => TurnDelivery::Incomplete,
        (true, false) => TurnDelivery::Failed(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_turn_error;
    use serde_json::{Value, json};

    #[test]
    fn error_redaction_keeps_structure_and_covers_nested_values() {
        let private = vec![
            "long-token".to_owned(),
            "token".to_owned(),
            "12345".to_owned(),
        ];
        let (result, restart) = normalize_turn_error(
            json!({
                "code": -32603,
                "message": "HTTP 404 from long-token: model not found",
                "data": {"token": ["long-token", 12345]}
            }),
            &private,
        );
        assert!(!restart);
        assert_eq!(result["code"], -32603);
        assert_eq!(
            result["message"],
            "HTTP 404 from [redacted]: model not found"
        );
        assert_eq!(result["data"]["[redacted]"][0], "[redacted]");
        assert_eq!(result["data"]["[redacted]"][1], "[redacted]");
        assert_eq!(
            normalize_turn_error(Value::Null, &private),
            (Value::Null, false)
        );
        let (_, restart) = normalize_turn_error(
            json!({"message":"Internal error: turn failed: Unauthorized tool action"}),
            &[],
        );
        assert!(!restart);
    }
}
