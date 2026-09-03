use super::events::{
    ActiveTurn, ToolCall, handle_notification, publish_approval_required, publish_started,
    settle_turn, status_from_stop_reason,
};
use super::framing::{parse_frame, write_message};
use super::pending::PendingKind;
use super::permissions::permission_response;
use super::{
    AcpCommand, AnalystEvent, MANAGED_AGENT_DISCONNECTED_METHOD, PermissionPolicy, PromptCompletion,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::process::ChildStdin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;

const MAX_PENDING_NOTIFICATION_BYTES: usize = 256 * 1024;
const MAX_PENDING_NOTIFICATIONS: usize = 512;
const POST_RESPONSE_QUIESCENCE: Duration = Duration::from_millis(150);
const POST_RESPONSE_MAX_DRAIN: Duration = Duration::from_secs(2);

struct DeferredSettlement {
    turn_id: String,
    status: &'static str,
    error: Option<String>,
    hard_deadline: Instant,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    stdin: Arc<Mutex<ChildStdin>>,
    mut commands: mpsc::Receiver<AcpCommand>,
    mut frames: mpsc::Receiver<io::Result<Vec<u8>>>,
    events: broadcast::Sender<AnalystEvent>,
    generation: String,
    runtime: &'static str,
    permission_policy: PermissionPolicy,
    prompt_completion: PromptCompletion,
) {
    let mut next_id = 1_u64;
    let mut pending: HashMap<u64, PendingKind> = HashMap::new();
    let mut session_id = String::new();
    let mut loading_request_id = None;
    let mut active: Option<ActiveTurn> = None;
    let mut cancelling_turn_id: Option<String> = None;
    let mut tool_calls = HashMap::<String, ToolCall>::new();
    let settle_timer = tokio::time::sleep(Duration::from_secs(24 * 60 * 60));
    tokio::pin!(settle_timer);
    let mut deferred_settlement: Option<DeferredSettlement> = None;
    let terminal_error = loop {
        tokio::select! {
            _ = &mut settle_timer, if deferred_settlement.is_some() => {
                let settlement = deferred_settlement
                    .take()
                    .expect("guarded deferred ACP settlement");
                settle_turn(
                    &events,
                    &generation,
                    &session_id,
                    &settlement.turn_id,
                    settlement.status,
                    settlement.error.as_deref(),
                    &mut active,
                );
                if cancelling_turn_id.as_deref() == Some(settlement.turn_id.as_str()) {
                    cancelling_turn_id = None;
                }
            }
            command = commands.recv() => match command {
                Some(AcpCommand::Request(request)) => {
                    let id = next_id;
                    next_id = next_id.saturating_add(1);
                    if request.method == "session/load" {
                        loading_request_id = Some(id);
                    }
                    let requested_session_id = (request.method == "session/load")
                        .then(|| request.params.get("sessionId")?.as_str().map(str::to_owned))
                        .flatten();
                    let message = json!({
                        "jsonrpc":"2.0", "id":id,
                        "method":request.method, "params":request.params,
                    });
                    let method = message["method"].as_str().unwrap_or_default().to_owned();
                    match write_message(&stdin, message).await {
                        Ok(()) => {
                            pending.insert(id, PendingKind::Request {
                                method,
                                requested_session_id,
                                response: request.response,
                            });
                        }
                        Err(error) => {
                            let detail = error.to_string();
                            let _ = request.response.send(Err(error));
                            break detail;
                        }
                    }
                }
                Some(AcpCommand::Prompt(request)) => {
                    if request.session_id != session_id || session_id.is_empty() {
                        let _ = request.response.send(Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "ACP prompt targets a different session",
                        )));
                        continue;
                    }
                    if active.is_some() {
                        let _ = request.response.send(Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "managed Agent already has an active turn",
                        )));
                        continue;
                    }
                    let id = next_id;
                    next_id = next_id.saturating_add(1);
                    let turn_id = format!("acp-{}", uuid::Uuid::new_v4().simple());
                    let message = json!({
                        "jsonrpc":"2.0", "id":id, "method":"session/prompt",
                        "params":{
                            "sessionId":session_id,
                            "prompt":[{"type":"text","text":request.text}],
                        }
                    });
                    match write_message(&stdin, message).await {
                        Ok(()) => {
                            pending.insert(id, PendingKind::Prompt {
                                turn_id: turn_id.clone(),
                                delegation_id: request.delegation_id,
                                expected_user_text: request.text,
                                observed_user_text: String::new(),
                                buffered_notifications: Vec::new(),
                                buffered_bytes: 0,
                                response: Some(request.response),
                            });
                            active = Some(ActiveTurn {
                                turn_id,
                                external: false,
                                admitted: false,
                            });
                        }
                        Err(error) => {
                            let detail = error.to_string();
                            let _ = request.response.send(Err(error));
                            break detail;
                        }
                    }
                }
                Some(AcpCommand::Cancel { session_id: requested, response }) => {
                    let active_turn_id = active.as_ref().map(|turn| turn.turn_id.clone());
                    let result = if requested != session_id || session_id.is_empty() {
                        Err(io::Error::new(io::ErrorKind::InvalidInput, "ACP cancel targets a different session"))
                    } else if active.is_none() {
                        Err(io::Error::new(io::ErrorKind::NotFound, "managed Agent has no active turn"))
                    } else {
                        write_message(&stdin, json!({
                            "jsonrpc":"2.0", "method":"session/cancel",
                            "params":{"sessionId":session_id},
                        })).await
                    };
                    if result.is_ok() {
                        cancelling_turn_id = active_turn_id;
                    }
                    let failed = result.as_ref().err().map(ToString::to_string);
                    let _ = response.send(result);
                    if let Some(detail) = failed {
                        break detail;
                    }
                }
                Some(AcpCommand::Respond { id, result }) => {
                    if let Err(error) = write_message(&stdin, json!({"jsonrpc":"2.0","id":id,"result":result})).await {
                        break error.to_string();
                    }
                }
                Some(AcpCommand::RespondError { id, error }) => {
                    if let Err(error) = write_message(&stdin, json!({"jsonrpc":"2.0","id":id,"error":error})).await {
                        break error.to_string();
                    }
                }
                Some(AcpCommand::ExternalStatus { session_id: observed, busy }) => {
                    if observed != session_id || session_id.is_empty() {
                        continue;
                    }
                    if busy {
                        if active.is_none() {
                            let turn_id = format!("acp-tui-{}", uuid::Uuid::new_v4().simple());
                            active = Some(ActiveTurn {
                                turn_id: turn_id.clone(),
                                external: true,
                                admitted: true,
                            });
                            publish_started(&events, &generation, &session_id, &turn_id, None);
                        }
                    } else if let Some(turn_id) = active
                        .as_ref()
                        .filter(|turn| turn.external)
                        .map(|turn| turn.turn_id.clone())
                    {
                        let status = if cancelling_turn_id.as_deref() == Some(turn_id.as_str()) {
                            "cancelled"
                        } else {
                            "completed"
                        };
                        settle_turn(
                            &events,
                            &generation,
                            &session_id,
                            &turn_id,
                            status,
                            None,
                            &mut active,
                        );
                        if cancelling_turn_id.as_deref() == Some(turn_id.as_str()) {
                            cancelling_turn_id = None;
                        }
                    }
                }
                Some(AcpCommand::ObservedUserText { session_id: observed, text }) => {
                    if observed != session_id || session_id.is_empty() {
                        continue;
                    }
                    let message = json!({
                        "method":"session/update",
                        "params":{
                            "sessionId":session_id,
                            "update":{
                                "sessionUpdate":"user_message_chunk",
                                "content":{"type":"text","text":text},
                            }
                        }
                    });
                    if let Some(turn_id) = admit_matching_prompt(
                        &message,
                        &mut pending,
                        &mut active,
                        &events,
                        &generation,
                        &session_id,
                    ) {
                        flush_buffered_notifications(
                            &turn_id,
                            &mut pending,
                            &events,
                            &generation,
                            &session_id,
                            &mut active,
                            &mut tool_calls,
                        );
                    }
                }
                Some(AcpCommand::ExternalDisconnected { reason }) => break reason,
                Some(AcpCommand::Close) | None => break "managed ACP client closed".to_owned(),
            },
            frame = frames.recv() => {
                let frame = match frame {
                    Some(Ok(frame)) => frame,
                    Some(Err(error)) => break error.to_string(),
                    None => break "ACP stdout reader closed".to_owned(),
                };
                let message = match parse_frame(&frame) {
                    Ok(message) => message,
                    Err(error) => break error.to_string(),
                };
                if let Some(method) = message.get("method").and_then(Value::as_str) {
                    if message.get("id").is_some() {
                        let id = message["id"].clone();
                        if method == "session/request_permission" {
                            let (result, allowed) =
                                permission_response(&message, permission_policy);
                            if let Err(error) = write_message(&stdin, json!({"jsonrpc":"2.0","id":id,"result":result})).await {
                                break error.to_string();
                            }
                            if !allowed {
                                publish_approval_required(
                                    &events,
                                    &generation,
                                    &session_id,
                                    runtime,
                                );
                            }
                            continue;
                        }
                        if let Err(error) = write_message(&stdin, json!({
                            "jsonrpc":"2.0", "id":id,
                            "error":{"code":-32601,"message":"unsupported ACP client method"}
                        })).await {
                            break error.to_string();
                        }
                        continue;
                    }
                    if loading_request_id.is_none() {
                        if let Some(turn_id) = admit_matching_prompt(
                            &message,
                            &mut pending,
                            &mut active,
                            &events,
                            &generation,
                            &session_id,
                        ) {
                            flush_buffered_notifications(
                                &turn_id,
                                &mut pending,
                                &events,
                                &generation,
                                &session_id,
                                &mut active,
                                &mut tool_calls,
                            );
                        }
                        match buffer_unadmitted_notification(&message, frame.len(), &mut pending, active.as_ref()) {
                            Ok(true) => continue,
                            Ok(false) => {}
                            Err(error) => break error,
                        }
                        handle_notification(
                            method,
                            &message,
                            &events,
                            &generation,
                            &session_id,
                            &mut active,
                            &mut tool_calls,
                        );
                        if extend_deferred_settlement(
                            &message,
                            active.as_ref(),
                            deferred_settlement.as_ref(),
                        ) {
                            let deadline = deferred_settlement
                                .as_ref()
                                .expect("extended deferred ACP settlement")
                                .hard_deadline
                                .min(Instant::now() + POST_RESPONSE_QUIESCENCE);
                            settle_timer.as_mut().reset(deadline);
                        }
                    }
                    continue;
                }
                let Some(id) = message.get("id").and_then(Value::as_u64) else {
                    break "ACP response has an invalid id".to_owned();
                };
                let Some(kind) = pending.remove(&id) else {
                    break format!("ACP returned an unknown response id: {id}");
                };
                if loading_request_id == Some(id) {
                    loading_request_id = None;
                }
                let result = if let Some(error) = message.get("error") {
                    Err(io::Error::other(format!("ACP request failed: {error}")))
                } else {
                    Ok(message.get("result").cloned().unwrap_or(Value::Null))
                };
                match kind {
                    PendingKind::Request { method, requested_session_id, response } => {
                        if let Ok(result) = &result
                            && matches!(method.as_str(), "session/new" | "session/load")
                        {
                            let returned = result
                                .get("sessionId")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_owned)
                                .or(requested_session_id);
                            let Some(returned) = returned else {
                                let _ = response.send(Err(io::Error::other("ACP returned an empty session id")));
                                continue;
                            };
                            session_id = returned;
                            active = None;
                            cancelling_turn_id = None;
                            tool_calls.clear();
                        }
                        let _ = response.send(result);
                    }
                    PendingKind::Prompt {
                        turn_id,
                        delegation_id,
                        buffered_notifications,
                        response,
                        ..
                    } => {
                        match result {
                            Err(error) if response.is_some() => {
                                clear_unadmitted_turn(&mut active, &turn_id);
                                let _ = response
                                    .expect("guarded pending ACP response")
                                    .send(Err(prompt_admission_error(error)));
                                continue;
                            }
                            outcome => {
                                if let Some(response) = response {
                                    mark_admitted(&mut active, &turn_id);
                                    publish_started(
                                        &events,
                                        &generation,
                                        &session_id,
                                        &turn_id,
                                        Some(delegation_id),
                                    );
                                    flush_notifications(
                                        buffered_notifications,
                                        &events,
                                        &generation,
                                        &session_id,
                                        &mut active,
                                        &mut tool_calls,
                                    );
                                    let _ = response.send(Ok(turn_id.clone()));
                                }
                                let cancellation_requested =
                                    cancelling_turn_id.as_deref() == Some(turn_id.as_str());
                                let (status, error) = if cancellation_requested {
                                    ("cancelled", None)
                                } else {
                                    match outcome {
                                        Ok(result) => status_from_stop_reason(
                                            result.get("stopReason").and_then(Value::as_str),
                                        ),
                                        Err(error) => ("failed", Some(error.to_string())),
                                    }
                                };
                                if prompt_completion == PromptCompletion::Response {
                                    settle_turn(
                                        &events,
                                        &generation,
                                        &session_id,
                                        &turn_id,
                                        status,
                                        error.as_deref(),
                                        &mut active,
                                    );
                                    if cancellation_requested {
                                        cancelling_turn_id = None;
                                    }
                                } else {
                                    let now = Instant::now();
                                    deferred_settlement = Some(DeferredSettlement {
                                        turn_id,
                                        status,
                                        error,
                                        hard_deadline: now + POST_RESPONSE_MAX_DRAIN,
                                    });
                                    settle_timer
                                        .as_mut()
                                        .reset(now + POST_RESPONSE_QUIESCENCE);
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    for (_, kind) in pending {
        match kind {
            PendingKind::Request { response, .. } => {
                let _ = response.send(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    terminal_error.clone(),
                )));
            }
            PendingKind::Prompt {
                response: Some(response),
                ..
            } => {
                let _ = response.send(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    terminal_error.clone(),
                )));
            }
            PendingKind::Prompt { response: None, .. } => {}
        }
    }
    let _ = events.send(AnalystEvent {
        generation,
        message: json!({
            "method":MANAGED_AGENT_DISCONNECTED_METHOD,
            "params":{"reason":terminal_error}
        }),
        requested_delegation_id: None,
    });
}

fn buffer_unadmitted_notification(
    message: &Value,
    frame_bytes: usize,
    pending: &mut HashMap<u64, PendingKind>,
    active: Option<&ActiveTurn>,
) -> Result<bool, String> {
    if !is_turn_activity(message) {
        return Ok(false);
    }
    let Some(turn_id) = active
        .filter(|turn| !turn.external && !turn.admitted)
        .map(|turn| turn.turn_id.as_str())
    else {
        return Ok(false);
    };
    let Some((notifications, bytes)) = pending.values_mut().find_map(|kind| match kind {
        PendingKind::Prompt {
            turn_id: pending_turn_id,
            buffered_notifications,
            buffered_bytes,
            response,
            ..
        } if pending_turn_id == turn_id && response.is_some() => {
            Some((buffered_notifications, buffered_bytes))
        }
        _ => None,
    }) else {
        return Ok(false);
    };
    if notifications.len() >= MAX_PENDING_NOTIFICATIONS
        || bytes.saturating_add(frame_bytes) > MAX_PENDING_NOTIFICATION_BYTES
    {
        return Err("ACP pre-admission notification buffer exceeded its bounded limit".to_owned());
    }
    notifications.push(message.clone());
    *bytes += frame_bytes;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn flush_buffered_notifications(
    turn_id: &str,
    pending: &mut HashMap<u64, PendingKind>,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    active: &mut Option<ActiveTurn>,
    tool_calls: &mut HashMap<String, ToolCall>,
) {
    let buffered = pending.values_mut().find_map(|kind| match kind {
        PendingKind::Prompt {
            turn_id: pending_turn_id,
            buffered_notifications,
            buffered_bytes,
            ..
        } if pending_turn_id == turn_id => {
            *buffered_bytes = 0;
            Some(std::mem::take(buffered_notifications))
        }
        _ => None,
    });
    if let Some(buffered) = buffered {
        flush_notifications(buffered, events, generation, session_id, active, tool_calls);
    }
}

fn flush_notifications(
    notifications: Vec<Value>,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    active: &mut Option<ActiveTurn>,
    tool_calls: &mut HashMap<String, ToolCall>,
) {
    for message in notifications {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        handle_notification(
            method, &message, events, generation, session_id, active, tool_calls,
        );
    }
}

fn is_turn_activity(message: &Value) -> bool {
    message.get("method").and_then(Value::as_str) == Some("session/update")
        && matches!(
            message
                .pointer("/params/update/sessionUpdate")
                .and_then(Value::as_str),
            Some(
                "agent_message_chunk"
                    | "agent_thought_chunk"
                    | "tool_call"
                    | "tool_call_update"
                    | "plan"
            )
        )
}

fn extend_deferred_settlement(
    message: &Value,
    active: Option<&ActiveTurn>,
    settlement: Option<&DeferredSettlement>,
) -> bool {
    let Some(settlement) = settlement else {
        return false;
    };
    active.is_some_and(|turn| turn.turn_id == settlement.turn_id)
        && is_turn_activity(message)
        && Instant::now() < settlement.hard_deadline
}

fn admit_matching_prompt(
    message: &Value,
    pending: &mut HashMap<u64, PendingKind>,
    active: &mut Option<ActiveTurn>,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
) -> Option<String> {
    if message.get("method").and_then(Value::as_str) != Some("session/update")
        || message.pointer("/params/sessionId").and_then(Value::as_str) != Some(session_id)
        || message
            .pointer("/params/update/sessionUpdate")
            .and_then(Value::as_str)
            != Some("user_message_chunk")
    {
        return None;
    }
    let chunk = message
        .pointer("/params/update/content/text")
        .and_then(Value::as_str)?;
    let turn_id = active
        .as_ref()
        .filter(|turn| !turn.external && !turn.admitted)
        .map(|turn| turn.turn_id.clone())?;
    let prompt = pending.values_mut().find_map(|kind| match kind {
        PendingKind::Prompt {
            turn_id: pending_turn_id,
            expected_user_text,
            observed_user_text,
            response,
            delegation_id,
            ..
        } if pending_turn_id == &turn_id && response.is_some() => Some((
            expected_user_text,
            observed_user_text,
            response,
            delegation_id.clone(),
        )),
        _ => None,
    })?;
    let (expected, observed, response, delegation_id) = prompt;
    let combined = format!("{observed}{chunk}");
    if expected.starts_with(&combined) {
        *observed = combined;
    } else if expected.starts_with(chunk) {
        *observed = chunk.to_owned();
    } else {
        observed.clear();
        return None;
    }
    if observed != expected {
        return None;
    }
    let response = response.take().expect("checked pending ACP admission");
    mark_admitted(active, &turn_id);
    publish_started(
        events,
        generation,
        session_id,
        &turn_id,
        Some(delegation_id),
    );
    let _ = response.send(Ok(turn_id.clone()));
    Some(turn_id)
}

fn mark_admitted(active: &mut Option<ActiveTurn>, turn_id: &str) {
    if let Some(turn) = active
        .as_mut()
        .filter(|turn| turn.turn_id == turn_id && !turn.external)
    {
        turn.admitted = true;
    }
}

fn clear_unadmitted_turn(active: &mut Option<ActiveTurn>, turn_id: &str) {
    if active
        .as_ref()
        .is_some_and(|turn| turn.turn_id == turn_id && !turn.admitted)
    {
        *active = None;
    }
}

fn prompt_admission_error(error: io::Error) -> io::Error {
    let message = error.to_string();
    let normalized = message.to_ascii_lowercase();
    let provider_busy = [
        "busy",
        "active prompt",
        "active turn",
        "already processing",
        "currently processing",
        "in progress",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    io::Error::new(
        if provider_busy {
            io::ErrorKind::WouldBlock
        } else {
            error.kind()
        },
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PENDING_NOTIFICATION_BYTES, admit_matching_prompt, buffer_unadmitted_notification,
        prompt_admission_error,
    };
    use crate::ops::codex_voice_analyst::acp::events::ActiveTurn;
    use crate::ops::codex_voice_analyst::acp::pending::PendingKind;
    use serde_json::json;
    use std::collections::HashMap;
    use std::io;
    use tokio::sync::{broadcast, oneshot};

    #[test]
    fn provider_busy_before_admission_is_retryable() {
        let error = prompt_admission_error(io::Error::other(
            r#"ACP request failed: {"code":-32603,"message":"session is busy"}"#,
        ));
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);

        let error = prompt_admission_error(io::Error::other(
            r#"ACP request failed: {"code":-32603,"message":"provider failed"}"#,
        ));
        assert_eq!(error.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn pre_admission_output_buffer_is_bounded() {
        let (response, _receipt) = oneshot::channel();
        let mut pending = HashMap::from([(
            7,
            PendingKind::Prompt {
                turn_id: "turn-1".into(),
                delegation_id: "delivery-1".into(),
                expected_user_text: "owned prompt".into(),
                observed_user_text: String::new(),
                buffered_notifications: Vec::new(),
                buffered_bytes: MAX_PENDING_NOTIFICATION_BYTES,
                response: Some(response),
            },
        )]);
        let active = ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: false,
        };
        let message = json!({
            "method":"session/update",
            "params":{
                "sessionId":"session-1",
                "update":{
                    "sessionUpdate":"agent_message_chunk",
                    "content":{"type":"text","text":"must not grow without bound"},
                }
            }
        });

        assert!(buffer_unadmitted_notification(&message, 1, &mut pending, Some(&active)).is_err());
    }

    #[test]
    fn only_the_matching_user_echo_admits_a_pending_prompt() {
        let (response, mut receipt) = oneshot::channel();
        let mut pending = HashMap::from([(
            7,
            PendingKind::Prompt {
                turn_id: "turn-1".into(),
                delegation_id: "delivery-1".into(),
                expected_user_text: "owned prompt".into(),
                observed_user_text: String::new(),
                buffered_notifications: Vec::new(),
                buffered_bytes: 0,
                response: Some(response),
            },
        )]);
        let mut active = Some(ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: false,
        });
        let (events, mut event_receiver) = broadcast::channel(4);
        let update = |text: &str| {
            json!({
                "method":"session/update",
                "params":{
                    "sessionId":"session-1",
                    "update":{
                        "sessionUpdate":"user_message_chunk",
                        "content":{"type":"text","text":text},
                    }
                }
            })
        };

        admit_matching_prompt(
            &update("terminal prompt"),
            &mut pending,
            &mut active,
            &events,
            "generation-1",
            "session-1",
        );
        assert!(!active.as_ref().expect("active").admitted);
        assert!(matches!(
            receipt.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(event_receiver.try_recv().is_err());

        admit_matching_prompt(
            &update("owned "),
            &mut pending,
            &mut active,
            &events,
            "generation-1",
            "session-1",
        );
        admit_matching_prompt(
            &update("prompt"),
            &mut pending,
            &mut active,
            &events,
            "generation-1",
            "session-1",
        );
        assert!(active.as_ref().expect("active").admitted);
        assert_eq!(
            receipt.try_recv().expect("admission").expect("accepted"),
            "turn-1"
        );
        let event = event_receiver.try_recv().expect("turn started");
        assert_eq!(event.message["method"], "turn/started");
        assert_eq!(event.requested_delegation_id.as_deref(), Some("delivery-1"));
    }
}
