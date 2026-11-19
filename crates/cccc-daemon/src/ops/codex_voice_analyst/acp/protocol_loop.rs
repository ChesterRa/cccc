use super::events::{
    ActiveTurn, ToolCall, handle_notification, publish_approval_required, publish_started,
    settle_turn, status_from_stop_reason,
};
use super::framing::{parse_frame, write_message};
use super::interactions;
use super::pending::PendingKind;
use super::permissions::{pending_request, permission_response};
use super::{
    AcpCommand, AnalystEvent, MANAGED_AGENT_DISCONNECTED_METHOD, PermissionPolicy, PrivateValues,
    PromptCompletion, TurnEnd,
};
use crate::ops::codex_voice_analyst::{MANAGED_AGENT_DELEGATION_ATTACHED_METHOD, native_input};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;

const MAX_PENDING_NOTIFICATION_BYTES: usize = 256 * 1024;
const MAX_PENDING_NOTIFICATIONS: usize = 512;
const POST_RESPONSE_QUIESCENCE: Duration = Duration::from_millis(150);
const POST_RESPONSE_MAX_DRAIN: Duration = Duration::from_secs(2);

pub(super) type SharedPromptReceipt =
    Arc<Mutex<Option<crate::ops::runtime_session::native_acp::PromptReceipt>>>;

struct DeferredSettlement {
    turn_id: String,
    status: &'static str,
    error: Option<String>,
    hard_deadline: Instant,
}

/// Clears the published open prompt however the loop ends, also when its task
/// is aborted while blocked on a write and never reaches its own clean-up.
struct OpenPromptGuard(Arc<AtomicBool>);

impl Drop for OpenPromptGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
    permissions: Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
    prompt_receipt: SharedPromptReceipt,
    private_values: PrivateValues,
    open_prompt: Arc<AtomicBool>,
) {
    let _open_prompt_cleared = OpenPromptGuard(Arc::clone(&open_prompt));
    let diagnostic_values = private_values.diagnostics();
    let mut next_id = 1_u64;
    let mut pending: HashMap<u64, PendingKind> = HashMap::new();
    let mut session_id = String::new();
    let mut loading_request_id = None;
    let mut active: Option<ActiveTurn> = None;
    let mut cancelling_turn_id: Option<String> = None;
    // Cancellation can reach this serial command loop just before its pending
    // prompt. Retire exactly that input without sending it to the provider.
    let mut cancelled_pending_input: Option<String> = None;
    // Set by stop's cancel; no prompt reaches the provider afterwards.
    let mut admission_closed = false;
    // The latest prompt of this client that answered with an explicit
    // stopReason. Only that proves the provider ended it.
    let mut stopped_turn: Option<String> = None;
    let mut tool_calls = HashMap::<String, ToolCall>::new();
    let mut native_inputs = VecDeque::new();
    let settle_timer = tokio::time::sleep(Duration::from_secs(24 * 60 * 60));
    tokio::pin!(settle_timer);
    let mut deferred_settlement: Option<DeferredSettlement> = None;
    let terminal_error = loop {
        // Published before each wait: a prompt of this client counts from the
        // completed write that made it active until it ends. A reader learns
        // whether a cancel had a target when it read; the cancel itself acts on
        // the turn current when this loop handles it.
        open_prompt.store(
            active.as_ref().is_some_and(|turn| !turn.external),
            Ordering::Release,
        );
        if active.is_none()
            && let Err(error) = expire_permissions(&stdin, &permissions, false).await
        {
            break error.to_string();
        }
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
                    if admission_closed {
                        let _ = request.response.send(Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "managed Agent is stopping; the input was not sent",
                        )));
                        continue;
                    }
                    if request.session_id != session_id || session_id.is_empty() {
                        let _ = request.response.send(Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "ACP prompt targets a different session",
                        )));
                        continue;
                    }
                    if cancelled_pending_input.as_deref() == Some(request.delegation_id.as_str()) {
                        cancelled_pending_input = None;
                        let _ = request.response.send(Err(io::Error::new(
                            io::ErrorKind::Interrupted, "ACP pending input was cancelled",
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
                    // Only a prompt about to be written marks its session as attempted.
                    let attempted = prompt_receipt
                        .lock()
                        .map_err(|_| io::Error::other("ACP prompt receipt lock failed"))
                        .and_then(|receipt| {
                            receipt.as_ref().map_or(Ok(()), |receipt| receipt.mark_attempted())
                        });
                    if let Err(error) = attempted {
                        let _ = request.response.send(Err(error));
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
                                rpc_completed: false,
                                response: Some(request.response),
                            });
                            active = Some(ActiveTurn {
                                turn_id,
                                external: false,
                                admitted: false,
                                provider_prompt_id: None,
                                provider_start_sequence: None,
                            });
                        }
                        Err(error) => {
                            let detail = error.to_string();
                            let _ = request.response.send(Err(error));
                            break detail;
                        }
                    }
                }
                Some(AcpCommand::Cancel { session_id: requested, delegation_id, close_admission, response }) => {
                    // A cancel that finds nothing, or names another session or input,
                    // is answered and leaves the session usable. Only a failed write
                    // to the provider ends this loop.
                    let active_turn_id = active.as_ref().map(|turn| turn.turn_id.clone());
                    if requested != session_id || session_id.is_empty() {
                        let _ = response.send(Err(io::Error::new(io::ErrorKind::InvalidInput, "ACP cancel targets a different session")));
                        continue;
                    }
                    // Commands are served in order: a prompt queued before this
                    // stop is the turn cancelled below; one queued after is refused.
                    admission_closed |= close_admission;
                    if active.is_none() {
                        if delegation_id.is_some() {
                            cancelled_pending_input = delegation_id;
                        }
                        let _ = response.send(Ok(None));
                        continue;
                    }
                    // A turn typed in the provider's own terminal is not this
                    // client's to cancel (Hermes cancels only its own prompts).
                    if close_admission && active.as_ref().is_some_and(|turn| turn.external) {
                        let _ = response.send(Ok(None));
                        continue;
                    }
                    if delegation_id.as_ref().is_some_and(|expected| {
                        !pending.values().any(|kind| matches!(kind,
                            PendingKind::Prompt { turn_id, delegation_id, .. }
                            if Some(turn_id) == active_turn_id.as_ref() && delegation_id == expected))
                    }) {
                        let _ = response.send(Err(io::Error::new(io::ErrorKind::InvalidInput, "ACP cancel targets a different pending input")));
                        continue;
                    }
                    if let Err(error) = expire_permissions(&stdin,&permissions,true).await {
                        let _ = response.send(Err(error));
                        break "ACP pending approvals could not be cancelled".into();
                    }
                    if let Err(error) = write_message(&stdin, json!({
                        "jsonrpc":"2.0", "method":"session/cancel",
                        "params":{"sessionId":session_id},
                    })).await {
                        let detail = error.to_string();
                        let _ = response.send(Err(error));
                        break detail;
                    }
                    cancelling_turn_id = active_turn_id.clone();
                    let _ = response.send(Ok(active_turn_id));
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
                Some(AcpCommand::Permission { request_id, reply, response }) => {
                    let request = permissions.lock().ok().and_then(|items|items.get(&request_id).cloned());
                    let Some(request) = request else {
                        let _ = response.send(Err(io::Error::new(io::ErrorKind::NotFound,"Permission request is no longer pending")));
                        continue;
                    };
                    if request["_cccc_turn_id"].as_str() != active.as_ref().map(|turn| turn.turn_id.as_str()) || cancelling_turn_id.is_some() {
                        let _ = response.send(Err(io::Error::new(io::ErrorKind::NotFound,"ACP interaction belongs to a retired turn")));
                        continue;
                    }
                    let result = if request["method"] == "session/request_permission" {
                        reply["allow"].as_bool().map(|allow| permission_response(&request, if allow { PermissionPolicy::AllowOnce } else { PermissionPolicy::Reject }).0).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"Tool permission requires boolean allow"))
                    } else { interactions::response(&request,&reply) };
                    let result = match result { Ok(result) => result, Err(error) => { let _ = response.send(Err(error)); continue; } };
                    let result = write_message(&stdin, json!({"jsonrpc":"2.0","id":request["id"],"result":result})).await;
                    let failed = result.is_err();
                    if result.is_ok() {
                        let still_waiting = if let Ok(mut items) = permissions.lock() { items.remove(&request_id); !items.is_empty() } else { false };
                        let flags = if still_waiting { vec!["waitingOnApproval"] } else { vec![] };
                        let _ = events.send(AnalystEvent {generation:generation.clone(),requested_delegation_id:None,message:json!({"method":"thread/status/changed","params":{"threadId":session_id,"status":{"type":"active","activeFlags":flags}}})});
                    }
                    let _ = response.send(result);
                    if failed { break "ACP permission response could not be delivered".into(); }
                }
                Some(AcpCommand::ExternalStatus { session_id: observed, busy, error }) => {
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
                                provider_prompt_id: None,
                                provider_start_sequence: None,
                            });
                            publish_started(&events, &generation, &session_id, &turn_id, None);
                        }
                    } else if let Some(turn_id) = active
                        .as_ref()
                        .filter(|turn| turn.admitted && (turn.external || prompt_completion == PromptCompletion::SessionEvents))
                        .map(|turn| turn.turn_id.clone())
                    {
                        let status = if cancelling_turn_id.as_deref() == Some(turn_id.as_str())
                            || error.as_ref().and_then(|error| error.get("name")).and_then(Value::as_str) == Some("MessageAbortedError") {
                            "cancelled"
                        } else if error.is_some() {
                            "failed"
                        } else {
                            "completed"
                        };
                        settle_turn(
                            &events,
                            &generation,
                            &session_id,
                            &turn_id,
                            status,
                            error.as_ref().and_then(|error| error.pointer("/data/message")).and_then(Value::as_str),
                            &mut active,
                        );
                        tool_calls.clear();
                        if cancelling_turn_id.as_deref() == Some(turn_id.as_str()) {
                            cancelling_turn_id = None;
                        }
                    }
                }
                Some(AcpCommand::SessionUpdate { session_id: observed, update }) => {
                    if prompt_completion != PromptCompletion::SessionEvents || observed != session_id {
                        continue;
                    }
                    let message = json!({"method":"session/update", "params":{"sessionId":session_id, "update":update}});
                    handle_notification("session/update", &message, &events, &generation, &session_id, &mut active, &mut tool_calls, diagnostic_values);
                }
                Some(AcpCommand::ObservedUserText { session_id: observed, text, consumed, response }) => {
                    let result = if observed != session_id || session_id.is_empty() {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "observed ACP user text targets a different session",
                        ))
                    } else {
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
                        let (controlled_echo, admitted_turn) = admit_matching_prompt(
                            &message,
                            &mut pending,
                            &mut active,
                            &events,
                            &generation,
                            &session_id,
                        );
                        if let Some(turn_id) = admitted_turn {
                            flush_buffered_notifications(
                                &turn_id,
                                &mut pending,
                                &events,
                                &generation,
                                &session_id,
                                &mut active,
                                &mut tool_calls,
                                diagnostic_values,
                            );
                        }
                        if !controlled_echo && consumed {
                            admit_matching_native_input(
                                &message,
                                &mut native_inputs,
                                &mut active,
                                &events,
                                &generation,
                                &session_id,
                                runtime,
                            );
                        }
                        pending.retain(|_, kind| !matches!(kind, PendingKind::Prompt { rpc_completed: true, response: None, .. }));
                        Ok(!controlled_echo)
                    };
                    let _ = response.send(result);
                }
                Some(AcpCommand::RegisterNativeInput { delegation_id, text, response }) => {
                    let _ = response.send(native_input::register(
                        &mut native_inputs,
                        delegation_id,
                        text,
                    ));
                }
                Some(AcpCommand::ForgetNativeInput { delegation_id, response }) => {
                    native_input::forget(&mut native_inputs, &delegation_id);
                    let _ = response.send(());
                }
                Some(AcpCommand::ExternalDisconnected { reason }) => break reason,
                Some(AcpCommand::TurnEnd { turn_id, response }) => {
                    let end = if stopped_turn.as_deref() == Some(turn_id.as_str()) {
                        TurnEnd::Stopped
                    } else if active.as_ref().is_some_and(|turn| turn.turn_id == turn_id) {
                        TurnEnd::Open
                    } else {
                        TurnEnd::Unknown
                    };
                    let _ = response.send(end);
                }
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
                if prompt_completion == PromptCompletion::ResponseWithActivityReceipt
                    && loading_request_id.is_none()
                    && message.pointer("/params/_meta/isReplay").and_then(Value::as_bool) != Some(true)
                    && ((message.pointer("/params/sessionId").and_then(Value::as_str) == Some(session_id.as_str())
                        && (is_turn_activity(&message) || message["method"] == "session/request_permission"))
                        || (interactions::is_cursor_interaction(&message,runtime) && active.is_some() && cancelling_turn_id.is_none()
                            && message.pointer("/params/sessionId").and_then(Value::as_str).is_none_or(|id| id == session_id)))
                {
                    admit_activity_prompt(&mut pending,&mut active,&events,&generation,&session_id);
                }
                if let Some(method) = message.get("method").and_then(Value::as_str) {
                    if message.get("id").is_some() {
                        let id = message["id"].clone();
                        if interactions::is_cursor_interaction(&message,runtime) {
                            let live = active.is_some() && loading_request_id.is_none() && cancelling_turn_id.is_none()
                                && message.pointer("/params/sessionId").and_then(Value::as_str).is_none_or(|id| id == session_id)
                                && message.pointer("/params/_meta/isReplay").and_then(Value::as_bool) != Some(true);
                            if !live {
                                if let Err(error) = write_message(&stdin,json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}})).await { break error.to_string(); }
                                continue;
                            }
                            if !interactions::valid_request(&message) {
                                if let Err(error) = write_message(&stdin,json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Malformed Cursor user interaction"}})).await { break error.to_string(); }
                                continue;
                            }
                            let (token, request) = pending_request(&message, &active.as_ref().expect("live Cursor interaction").turn_id);
                            if let Ok(mut items) = permissions.lock() { items.insert(token,request); }
                            let _ = events.send(AnalystEvent {generation:generation.clone(),requested_delegation_id:None,message:json!({"method":"thread/status/changed","params":{"threadId":session_id,"status":{"type":"active","activeFlags":["waitingOnApproval"]}}})});
                            let _ = events.send(AnalystEvent {generation:generation.clone(),requested_delegation_id:None,message:json!({"method":"cccc/approvalRequired","params":{"threadId":session_id}})});
                            continue;
                        }
                        if method == "session/request_permission" {
                            if active.is_none() || loading_request_id.is_some() || cancelling_turn_id.is_some()
                                || message.pointer("/params/sessionId").and_then(Value::as_str) != Some(session_id.as_str())
                                || message.pointer("/params/_meta/isReplay").and_then(Value::as_bool) == Some(true) {
                                if let Err(error) = write_message(&stdin,json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}})).await { break error.to_string(); }
                                continue;
                            }
                            if permission_policy == PermissionPolicy::Interactive {
                                if active.is_none() || message.pointer("/params/sessionId").and_then(Value::as_str) != Some(session_id.as_str()) || message.pointer("/params/_meta/isReplay").and_then(Value::as_bool) == Some(true) {
                                    if let Err(error) = write_message(&stdin,json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}})).await { break error.to_string(); }
                                    continue;
                                }
                                let (token, permission) = pending_request(&message, &active.as_ref().expect("live ACP permission").turn_id);
                                if let Ok(mut items) = permissions.lock() { items.insert(token, permission); }
                                let _ = events.send(AnalystEvent {generation:generation.clone(),requested_delegation_id:None,message:json!({"method":"thread/status/changed","params":{"threadId":session_id,"status":{"type":"active","activeFlags":["waitingOnApproval"]}}})});
                                let _ = events.send(AnalystEvent {generation:generation.clone(),requested_delegation_id:None,message:json!({"method":"cccc/approvalRequired","params":{"threadId":session_id}})});
                                continue;
                            }
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
                    if prompt_completion != PromptCompletion::SessionEvents
                        && loading_request_id.is_none()
                        && message.pointer("/params/_meta/isReplay").and_then(Value::as_bool) != Some(true)
                    {
                        let (controlled_echo, admitted_turn) = admit_matching_prompt(
                            &message,
                            &mut pending,
                            &mut active,
                            &events,
                            &generation,
                            &session_id,
                        );
                        if let Some(turn_id) = admitted_turn {
                            flush_buffered_notifications(
                                &turn_id,
                                &mut pending,
                                &events,
                                &generation,
                                &session_id,
                                &mut active,
                                &mut tool_calls,
                                diagnostic_values,
                            );
                        }
                        if !controlled_echo {
                            admit_matching_native_input(
                                &message,
                                &mut native_inputs,
                                &mut active,
                                &events,
                                &generation,
                                &session_id,
                                runtime,
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
                            diagnostic_values,
                        );
                        if active.is_none() {
                            deferred_settlement = None;
                            cancelling_turn_id = None;
                            tool_calls.clear();
                        }
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
                let Some(mut kind) = pending.remove(&id) else {
                    break format!("ACP returned an unknown response id: {id}");
                };
                if loading_request_id == Some(id) {
                    loading_request_id = None;
                }
                let result = if let Some(error) = message.get("error") {
                    let method = match &kind {
                        PendingKind::Request { method, .. } => method.as_str(),
                        PendingKind::Prompt { .. } => "session/prompt",
                    };
                    Err(rpc_error(runtime, method, error, &private_values))
                } else {
                    Ok(message.get("result").cloned().unwrap_or(Value::Null))
                };
                // A successful prompt RPC can overtake the independent SSE reader.
                // Keep its admission receipt until the persisted user echo arrives;
                // it cannot complete work observed on that stream.
                if prompt_completion == PromptCompletion::SessionEvents && result.is_ok()
                    && let PendingKind::Prompt { rpc_completed, response, .. } = &mut kind
                {
                    if response.is_some() {
                        *rpc_completed = true;
                        pending.insert(id, kind);
                    }
                    continue;
                }
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
                                let cancelled = cancelling_turn_id.as_deref() == Some(turn_id.as_str());
                                let error = if cancelled {
                                    cancelling_turn_id = None;
                                    io::Error::new(io::ErrorKind::Interrupted, "ACP pending input was cancelled")
                                } else {
                                    error
                                };
                                let _ = response
                                    .expect("guarded pending ACP response")
                                    .send(Err(error));
                                continue;
                            }
                            outcome => {
                                if outcome.as_ref().is_ok_and(|result| {
                                    result
                                        .get("stopReason")
                                        .and_then(Value::as_str)
                                        .is_some_and(|reason| !reason.trim().is_empty())
                                }) {
                                    stopped_turn = Some(turn_id.clone());
                                }
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
                                        diagnostic_values,
                                    );
                                    let _ = response.send(Ok(turn_id.clone()));
                                }
                                let cancellation_requested =
                                    cancelling_turn_id.as_deref() == Some(turn_id.as_str());
                                // Some agents answer a cancel with end_turn, and a turn
                                // can end while the cancel is in flight, so a requested
                                // cancel keeps a completed answer as cancelled. An error,
                                // another stop reason or none at all is what the provider
                                // reported, not a confirmed cancel.
                                let (status, error) = match outcome {
                                    Ok(result) => match status_from_stop_reason(
                                        result.get("stopReason").and_then(Value::as_str),
                                        diagnostic_values,
                                    ) {
                                        ("completed", _) if cancellation_requested => {
                                            ("cancelled", None)
                                        }
                                        settled => settled,
                                    },
                                    Err(error) => ("failed", Some(error.to_string())),
                                };
                                if prompt_completion != PromptCompletion::BoundedPostResponseDrain {
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
                                    if active.as_ref().is_none_or(|turn| turn.turn_id != turn_id) {
                                        continue;
                                    }
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
    open_prompt.store(false, Ordering::Release);
    if let Ok(mut items) = permissions.lock() {
        items.clear();
    }
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
    private_values: &[String],
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
        flush_notifications(
            buffered,
            events,
            generation,
            session_id,
            active,
            tool_calls,
            private_values,
        );
    }
}

fn flush_notifications(
    notifications: Vec<Value>,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    active: &mut Option<ActiveTurn>,
    tool_calls: &mut HashMap<String, ToolCall>,
    private_values: &[String],
) {
    for message in notifications {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        handle_notification(
            method,
            &message,
            events,
            generation,
            session_id,
            active,
            tool_calls,
            private_values,
        );
    }
}

async fn expire_permissions(
    stdin: &Arc<Mutex<ChildStdin>>,
    permissions: &Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
    all: bool,
) -> io::Result<()> {
    let expired = {
        let mut items = permissions
            .lock()
            .map_err(|_| io::Error::other("ACP permission state lock poisoned"))?;
        let keys = items
            .iter()
            .filter(|(_, message)| all || message["_cccc_turn_id"].is_string())
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| items.remove(&key))
            .collect::<Vec<_>>()
    };
    for message in expired {
        write_message(stdin,json!({"jsonrpc":"2.0","id":message["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await?;
    }
    Ok(())
}

fn admit_activity_prompt(
    pending: &mut HashMap<u64, PendingKind>,
    active: &mut Option<ActiveTurn>,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
) {
    let Some(turn) = active
        .as_mut()
        .filter(|turn| !turn.admitted && !turn.external)
    else {
        return;
    };
    let Some((response, delegation)) = pending.values_mut().find_map(|kind| match kind {
        PendingKind::Prompt {
            turn_id,
            response,
            delegation_id,
            ..
        } if turn_id == &turn.turn_id => Some((response, delegation_id.clone())),
        _ => None,
    }) else {
        return;
    };
    turn.admitted = true;
    publish_started(
        events,
        generation,
        session_id,
        &turn.turn_id,
        Some(delegation),
    );
    if let Some(response) = response.take() {
        let _ = response.send(Ok(turn.turn_id.clone()));
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
) -> (bool, Option<String>) {
    if message.get("method").and_then(Value::as_str) != Some("session/update")
        || message.pointer("/params/sessionId").and_then(Value::as_str) != Some(session_id)
        || message
            .pointer("/params/update/sessionUpdate")
            .and_then(Value::as_str)
            != Some("user_message_chunk")
    {
        return (false, None);
    }
    let Some(chunk) = message
        .pointer("/params/update/content/text")
        .and_then(Value::as_str)
    else {
        return (false, None);
    };
    let Some(turn_id) = active
        .as_ref()
        .filter(|turn| !turn.external && !turn.admitted)
        .map(|turn| turn.turn_id.clone())
    else {
        return (false, None);
    };
    let Some(prompt) = pending.values_mut().find_map(|kind| match kind {
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
    }) else {
        return (false, None);
    };
    let (expected, observed, response, delegation_id) = prompt;
    let combined = format!("{observed}{chunk}");
    if expected.starts_with(&combined) {
        *observed = combined;
    } else if expected.starts_with(chunk) {
        *observed = chunk.to_owned();
    } else {
        observed.clear();
        return (false, None);
    }
    if observed != expected {
        return (true, None);
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
    (true, Some(turn_id))
}

fn admit_matching_native_input(
    message: &Value,
    native_inputs: &mut VecDeque<native_input::PendingNativeInput>,
    active: &mut Option<ActiveTurn>,
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    session_id: &str,
    runtime: &str,
) {
    if message.get("method").and_then(Value::as_str) != Some("session/update")
        || message.pointer("/params/sessionId").and_then(Value::as_str) != Some(session_id)
        || message
            .pointer("/params/update/sessionUpdate")
            .and_then(Value::as_str)
            != Some("user_message_chunk")
    {
        return;
    }
    let Some(text) = message
        .pointer("/params/update/content/text")
        .and_then(Value::as_str)
    else {
        return;
    };
    // Grok wraps native interjections before echoing them. Only unwrap its
    // explicitly marked, complete envelope; ownership still requires an exact
    // match of the original payload, never a substring or a queue position.
    let text = if runtime == "grok"
        && message
            .pointer("/params/update/_meta/interjection")
            .and_then(Value::as_bool)
            == Some(true)
    {
        text.strip_prefix("The user sent a message while you were working:\n<user_query>\n")
            .and_then(|text| text.strip_suffix("\n</user_query>\nIf the user is asking for a response, address the user first. After replying, complete any unfinished tasks from previous turns."))
            .unwrap_or(text)
    } else {
        text
    };
    let Some(delegation_id) = native_input::observe(native_inputs, text) else {
        return;
    };
    if let Some(turn_id) = active.as_ref().map(|turn| turn.turn_id.clone()) {
        let _ = events.send(AnalystEvent {
            generation: generation.to_owned(),
            message: json!({
                "method":MANAGED_AGENT_DELEGATION_ATTACHED_METHOD,
                "params":{"threadId":session_id,"turnId":turn_id}
            }),
            requested_delegation_id: Some(delegation_id),
        });
        return;
    }
    let turn_id = format!("acp-tui-{}", uuid::Uuid::new_v4().simple());
    *active = Some(ActiveTurn {
        turn_id: turn_id.clone(),
        external: true,
        admitted: true,
        provider_prompt_id: None,
        provider_start_sequence: None,
    });
    publish_started(
        events,
        generation,
        session_id,
        &turn_id,
        Some(delegation_id),
    );
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

fn prompt_admission_kind(message: &str) -> io::ErrorKind {
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
    if provider_busy {
        io::ErrorKind::WouldBlock
    } else {
        io::ErrorKind::Other
    }
}

fn rpc_error(
    runtime: &str,
    method: &str,
    error: &Value,
    private_values: &PrivateValues,
) -> io::Error {
    let reported_message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Provider returned an error");
    let diagnostic_values = private_values.diagnostics();
    let message = super::diagnostic::sanitize(reported_message, diagnostic_values);
    // Only error.message supplies evidence of a busy rejection. Use the
    // admission policy selected by the adapter, regardless of the diagnostic.
    let kind = if method == "session/prompt" {
        match private_values {
            PrivateValues::DiagnosticsOnly(_) => {
                prompt_admission_kind(&super::diagnostic::sanitize(reported_message, &[]))
            }
            PrivateValues::DiagnosticsAndAdmission(_) => prompt_admission_kind(&message),
        }
    } else {
        io::ErrorKind::Other
    };
    let prefix = error.get("code").and_then(Value::as_i64).map_or_else(
        || format!("ACP {method} failed"),
        |code| format!("ACP {method} failed (code {code})"),
    );
    let details = (runtime == "hermes")
        .then(|| error.pointer("/data/details").and_then(Value::as_str))
        .flatten()
        .map(|value| super::diagnostic::sanitize(value, diagnostic_values))
        .filter(|value| !value.is_empty() && value != &message);
    let diagnostic = if let Some(details) = details {
        format!("{prefix}: {message}; {details}")
    } else {
        format!("{prefix}: {message}")
    };
    io::Error::new(
        kind,
        diagnostic
            .chars()
            .take(super::diagnostic::LIMIT)
            .collect::<String>(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PENDING_NOTIFICATION_BYTES, PrivateValues, admit_matching_native_input,
        admit_matching_prompt, buffer_unadmitted_notification, rpc_error,
    };
    use crate::ops::codex_voice_analyst::acp::events::ActiveTurn;
    use crate::ops::codex_voice_analyst::acp::pending::PendingKind;
    use serde_json::json;
    use std::collections::HashMap;
    use std::io;
    use tokio::sync::{broadcast, oneshot};

    #[test]
    fn provider_busy_before_admission_is_retryable() {
        let private_values = PrivateValues::DiagnosticsAndAdmission(Vec::new());
        let error = rpc_error(
            "hermes",
            "session/prompt",
            &json!({"code":-32603,"message":"session is busy"}),
            &private_values,
        );
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);

        let error = rpc_error(
            "hermes",
            "session/prompt",
            &json!({"code":-32603,"message":"provider failed"}),
            &private_values,
        );
        assert_eq!(error.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn diagnostic_only_private_values_preserve_prompt_admission() {
        let message = json!({"code":-32603,"message":"session is busy"});
        for (private, diagnostic) in [
            ("y", "session is bus[redacted]"),
            ("busy", "session is [redacted]"),
        ] {
            let error = rpc_error(
                "antigravity",
                "session/prompt",
                &message,
                &PrivateValues::DiagnosticsOnly(vec![private.into()]),
            );
            assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
            assert!(error.to_string().ends_with(diagnostic));
        }

        let error = rpc_error(
            "hermes",
            "session/prompt",
            &message,
            &PrivateValues::DiagnosticsAndAdmission(vec!["y".into()]),
        );
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().ends_with("session is bus[redacted]"));
    }

    #[test]
    fn diagnostic_only_private_values_still_filter_credential_shapes_for_admission() {
        let error = rpc_error(
            "grok",
            "session/prompt",
            &json!({"code":-32603,"message":"Internal error: token=private-busy-value"}),
            &PrivateValues::DiagnosticsOnly(vec!["y".into()]),
        );
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().ends_with("Internal error: [redacted]"));
    }

    #[test]
    fn explanatory_details_and_private_values_cannot_make_a_failed_prompt_retryable() {
        let error = rpc_error(
            "hermes",
            "session/prompt",
            &json!({"code":-32603,"message":"Internal error: private-busy-value","data":{"details":"Refresh request denied because its service is busy"}}),
            &PrivateValues::DiagnosticsAndAdmission(vec!["private-busy-value".into()]),
        );
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("service is busy"));
        assert!(!error.to_string().contains("private-busy-value"));
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
                rpc_completed: false,
                response: Some(response),
            },
        )]);
        let active = ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: false,
            provider_prompt_id: None,
            provider_start_sequence: None,
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
                rpc_completed: false,
                response: Some(response),
            },
        )]);
        let mut active = Some(ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: false,
            provider_prompt_id: None,
            provider_start_sequence: None,
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

    #[test]
    fn native_input_echo_attaches_to_the_active_turn_without_starting_another() {
        let mut native_inputs = std::collections::VecDeque::new();
        crate::ops::codex_voice_analyst::native_input::register(
            &mut native_inputs,
            "voice-2".into(),
            "new constraint".into(),
        )
        .expect("register native input");
        let mut active = Some(ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: true,
            provider_prompt_id: None,
            provider_start_sequence: None,
        });
        let (events, mut receiver) = broadcast::channel(4);
        let message = json!({
            "method":"session/update",
            "params":{
                "sessionId":"session-1",
                "update":{
                    "sessionUpdate":"user_message_chunk",
                    "content":{"type":"text","text":"new constraint"},
                }
            }
        });

        admit_matching_native_input(
            &message,
            &mut native_inputs,
            &mut active,
            &events,
            "generation-1",
            "session-1",
            "grok",
        );

        assert_eq!(active.as_ref().expect("active").turn_id, "turn-1");
        let associated = receiver.try_recv().expect("association event");
        assert_eq!(
            associated.message["method"],
            crate::ops::codex_voice_analyst::MANAGED_AGENT_DELEGATION_ATTACHED_METHOD
        );
        assert_eq!(associated.message["params"]["turnId"], "turn-1");
        assert_eq!(
            associated.requested_delegation_id.as_deref(),
            Some("voice-2")
        );
        assert!(native_inputs.is_empty());
    }

    #[test]
    fn queued_native_input_becomes_voice_owned_when_the_runtime_dequeues_it() {
        let mut native_inputs = std::collections::VecDeque::new();
        crate::ops::codex_voice_analyst::native_input::register(
            &mut native_inputs,
            "voice-queued".into(),
            "next investigation".into(),
        )
        .expect("register native input");
        let mut active = None;
        let (events, mut receiver) = broadcast::channel(4);
        let message = json!({
            "method":"session/update",
            "params":{
                "sessionId":"session-1",
                "update":{
                    "sessionUpdate":"user_message_chunk",
                    "content":{"type":"text","text":"next investigation"},
                }
            }
        });

        admit_matching_native_input(
            &message,
            &mut native_inputs,
            &mut active,
            &events,
            "generation-1",
            "session-1",
            "grok",
        );

        let turn = active.as_ref().expect("dequeued Runtime turn");
        assert!(turn.external);
        assert!(turn.admitted);
        let started = receiver.try_recv().expect("Voice-owned turn start");
        assert_eq!(started.message["method"], "turn/started");
        assert_eq!(started.message["params"]["turn"]["id"], turn.turn_id);
        assert_eq!(
            started.requested_delegation_id.as_deref(),
            Some("voice-queued")
        );
        assert!(native_inputs.is_empty());
    }

    #[test]
    fn grok_interjection_and_later_turn_keep_their_owners_after_a_missing_echo() {
        let mut native_inputs = std::collections::VecDeque::new();
        for (id, text) in [
            ("missing", "an input without an observed echo"),
            ("interjection", "inspect the synthetic fixture"),
            ("next", "inspect the next fixture"),
        ] {
            crate::ops::codex_voice_analyst::native_input::register(
                &mut native_inputs,
                id.into(),
                text.into(),
            )
            .expect("register native input");
        }
        let mut active = Some(ActiveTurn {
            turn_id: "current-turn".into(),
            external: true,
            admitted: true,
            provider_prompt_id: None,
            provider_start_sequence: None,
        });
        let (events, mut receiver) = broadcast::channel(16);
        let message = json!({
            "method":"session/update",
            "params":{"sessionId":"session-1","update":{
                "sessionUpdate":"user_message_chunk",
                "_meta":{"interjection":true},
                "content":{"type":"text","text":concat!(
                    "The user sent a message while you were working:\n<user_query>\n",
                    "inspect the synthetic fixture",
                    "\n</user_query>\nIf the user is asking for a response, address the user first. After replying, complete any unfinished tasks from previous turns."
                )}
            }}
        });
        admit_matching_native_input(
            &message,
            &mut native_inputs,
            &mut active,
            &events,
            "generation-1",
            "session-1",
            "grok",
        );
        let associated = receiver
            .try_recv()
            .expect("the interjection must be associated");
        assert_eq!(
            associated.requested_delegation_id.as_deref(),
            Some("interjection")
        );
        assert_eq!(associated.message["params"]["turnId"], "current-turn");
        super::settle_turn(
            &events,
            "generation-1",
            "session-1",
            "current-turn",
            "completed",
            None,
            &mut active,
        );
        let mut completed = false;
        while let Ok(event) = receiver.try_recv() {
            if event.message["method"] == "turn/completed" {
                assert_eq!(event.message["params"]["turn"]["id"], "current-turn");
                completed = true;
            }
        }
        assert!(completed, "the preceding turn must settle");
        let next = json!({
            "method":"session/update",
            "params":{"sessionId":"session-1","update":{
                "sessionUpdate":"user_message_chunk",
                "content":{"type":"text","text":"inspect the next fixture"}
            }}
        });
        admit_matching_native_input(
            &next,
            &mut native_inputs,
            &mut active,
            &events,
            "generation-1",
            "session-1",
            "grok",
        );
        let started = receiver.try_recv().expect("the following turn must start");
        assert_eq!(started.requested_delegation_id.as_deref(), Some("next"));
        assert_eq!(started.message["method"], "turn/started");
        let turn_id = active
            .as_ref()
            .expect("confirmed native turn")
            .turn_id
            .clone();
        let output = json!({
            "method":"session/update",
            "params":{"sessionId":"session-1","update":{
                "sessionUpdate":"agent_message_chunk",
                "content":{"type":"text","text":"the synthetic answer"}
            }}
        });
        super::handle_notification(
            "session/update",
            &output,
            &events,
            "generation-1",
            "session-1",
            &mut active,
            &mut HashMap::new(),
            &[],
        );
        let result = receiver.try_recv().expect("the following result must flow");
        assert_eq!(result.message["params"]["turnId"], turn_id);
        assert_eq!(result.message["params"]["delta"], "the synthetic answer");
        assert_eq!(native_inputs.len(), 1);
        assert_eq!(native_inputs[0].delegation_id, "missing");
    }

    #[test]
    fn controlled_echo_cannot_consume_an_identical_queued_native_input() {
        let (response, _receipt) = oneshot::channel();
        let mut pending = HashMap::from([(
            7,
            PendingKind::Prompt {
                turn_id: "turn-1".into(),
                delegation_id: "voice-1".into(),
                expected_user_text: "repeat this".into(),
                observed_user_text: String::new(),
                buffered_notifications: Vec::new(),
                buffered_bytes: 0,
                rpc_completed: false,
                response: Some(response),
            },
        )]);
        let mut native_inputs = std::collections::VecDeque::new();
        crate::ops::codex_voice_analyst::native_input::register(
            &mut native_inputs,
            "voice-2".into(),
            "repeat this".into(),
        )
        .expect("register native input");
        let mut active = Some(ActiveTurn {
            turn_id: "turn-1".into(),
            external: false,
            admitted: false,
            provider_prompt_id: None,
            provider_start_sequence: None,
        });
        let (events, _receiver) = broadcast::channel(4);
        let message = json!({
            "method":"session/update",
            "params":{
                "sessionId":"session-1",
                "update":{
                    "sessionUpdate":"user_message_chunk",
                    "content":{"type":"text","text":"repeat this"},
                }
            }
        });

        let (controlled_echo, admitted_turn) = admit_matching_prompt(
            &message,
            &mut pending,
            &mut active,
            &events,
            "generation-1",
            "session-1",
        );
        if !controlled_echo {
            admit_matching_native_input(
                &message,
                &mut native_inputs,
                &mut active,
                &events,
                "generation-1",
                "session-1",
                "grok",
            );
        }

        assert_eq!(admitted_turn.as_deref(), Some("turn-1"));
        assert_eq!(native_inputs.len(), 1);
        assert_eq!(native_inputs[0].delegation_id, "voice-2");
    }

    #[test]
    fn interjection_unwrapping_requires_grok_metadata_and_the_exact_envelope() {
        let wrapped = concat!(
            "The user sent a message while you were working:\n<user_query>\n",
            "inspect the fixture",
            "\n</user_query>\nIf the user is asking for a response, address the user first. After replying, complete any unfinished tasks from previous turns."
        );
        for (runtime, marked, text) in [
            ("kimi", true, wrapped.to_owned()),
            ("grok", false, wrapped.to_owned()),
            ("grok", true, format!("{wrapped} extra content")),
        ] {
            let mut pending = std::collections::VecDeque::new();
            crate::ops::codex_voice_analyst::native_input::register(
                &mut pending,
                "input".into(),
                "inspect the fixture".into(),
            )
            .expect("register native input");
            let message = json!({
                "method":"session/update","params":{"sessionId":"session-1","update":{
                    "sessionUpdate":"user_message_chunk","_meta":{"interjection":marked},
                    "content":{"type":"text","text":text}
                }}
            });
            let (events, mut receiver) = broadcast::channel(4);
            let mut active = None;
            admit_matching_native_input(
                &message,
                &mut pending,
                &mut active,
                &events,
                "generation-1",
                "session-1",
                runtime,
            );
            assert!(receiver.try_recv().is_err());
            assert_eq!(pending.len(), 1);
            assert!(active.is_none());
        }
    }
}
