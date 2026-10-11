use super::{
    Command, MANAGED_AGENT_DISCONNECTED_METHOD, Value, closed_error,
    events::{self, Turn},
    framing, interactions, json,
};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::process::ChildStdin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use tokio::sync::{broadcast, mpsc, oneshot};

enum Pending {
    Request {
        command: String,
        response: oneshot::Sender<io::Result<Value>>,
    },
    Prompt {
        turn_id: String,
    },
    CancelClear {
        turn_id: String,
        response: oneshot::Sender<io::Result<Option<String>>>,
    },
    CancelAbort {
        turn_id: String,
        provider_started: bool,
        response: oneshot::Sender<io::Result<Option<String>>>,
    },
    RejectQueue,
}
struct Interaction {
    record: Value,
    turn_id: String,
    expires: Option<Instant>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    stdin: Arc<Mutex<ChildStdin>>,
    mut commands: mpsc::Receiver<Command>,
    mut frames: mpsc::Receiver<io::Result<Value>>,
    events: broadcast::Sender<super::AnalystEvent>,
    generation: String,
    session_id: String,
    permissions: Arc<Mutex<BTreeMap<String, Value>>>,
    running: Arc<AtomicBool>,
    prompt_receipt: Arc<
        Mutex<Option<super::super::super::runtime_session::native_acp::PromptReceipt>>,
    >,
) {
    let mut pending = HashMap::<String, Pending>::new();
    let mut active: Option<Turn> = None;
    let mut dialogs = BTreeMap::<String, Interaction>::new();
    let mut serial = 0_u64;
    let reason = 'protocol: loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Request(request)) => {
                    if pending.len() >= 64 {
                        let _ = request.response.send(Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Pi RPC request capacity is full",
                        )));
                        continue;
                    }
                    let id = next_id(&generation, &mut serial);
                    let mut params = request.params.as_object().cloned().unwrap_or_default();
                    params.insert("type".into(), json!(request.command));
                    params.insert("id".into(), json!(id));
                    if framing::write(&stdin, Value::Object(params)).await.is_err() {
                        let _ = request.response.send(Err(closed_error()));
                        break 'protocol "Pi RPC command write failed";
                    }
                    pending.insert(
                        id,
                        Pending::Request {
                            command: request.command,
                            response: request.response,
                        },
                    );
                }
                Some(Command::Prompt {
                    session_id: requested,
                    delegation_id,
                    text,
                    response,
                }) => {
                    if requested != session_id || delegation_id.is_empty() || text.trim().is_empty() {
                        let _ = response.send(Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Pi controlled prompt identity or text is invalid",
                        )));
                        continue;
                    }
                    if active.is_some()
                        || pending.values().any(|request| {
                            matches!(
                                request,
                                Pending::CancelClear { .. }
                                    | Pending::CancelAbort { .. }
                                    | Pending::RejectQueue
                            )
                        })
                    {
                        let _ = response.send(Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Pi already has an active, pending or cancelling owned turn",
                        )));
                        continue;
                    }
                    let attempted = prompt_receipt
                        .lock()
                        .map_err(|_| io::Error::other("Pi prompt receipt lock is unavailable"))
                        .and_then(|receipt| {
                            receipt
                                .as_ref()
                                .map_or(Ok(()), |receipt| receipt.mark_attempted())
                        });
                    if let Err(error) = attempted {
                        let _ = response.send(Err(error));
                        continue;
                    }
                    let turn = Turn::new(delegation_id, text, response);
                    let id = next_id(&generation, &mut serial);
                    if framing::write(&stdin, json!({"id":id,"type":"prompt","message":turn.text}))
                        .await
                        .is_err()
                    {
                        if let Some(response) = turn.admission {
                            let _ = response.send(Err(closed_error()));
                        }
                        break 'protocol "Pi prompt write failed";
                    }
                    pending.insert(
                        id,
                        Pending::Prompt {
                            turn_id: turn.id.clone(),
                        },
                    );
                    active = Some(turn);
                }
                Some(Command::Cancel {
                    session_id: requested,
                    delegation_id,
                    turn_id,
                    response,
                }) => {
                    if requested != session_id
                        || active.as_ref().is_some_and(|turn| {
                            delegation_id
                                .as_ref()
                                .is_some_and(|id| id != &turn.delegation_id)
                                || turn_id.as_ref().is_some_and(|id| id != &turn.id)
                        })
                    {
                        let _ = response.send(Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Pi has no matching owned turn",
                        )));
                        continue;
                    }
                    let Some(turn) = active.as_mut() else {
                        let _ = response.send(Ok(None));
                        continue;
                    };
                    if turn.cancelling {
                        let _ = response.send(Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Pi cancellation is already pending",
                        )));
                        continue;
                    }
                    turn.cancelling = true;
                    if let Some(admission) = turn.admission.take() {
                        let _ = admission.send(Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "Pi prompt admission was cancelled",
                        )));
                    }
                    // A cancelled UI dialog must not keep preflight blocked. Retire
                    // its client token before sending any provider response.
                    let retired = std::mem::take(&mut dialogs);
                    clear_permissions(&permissions);
                    for (_, dialog) in retired {
                        if framing::write(&stdin,json!({"type":"extension_ui_response","id":dialog.record["id"],"cancelled":true})).await.is_err() {
                                    let _=response.send(Err(closed_error()));break 'protocol "Pi dialog cancellation failed";
                                }
                    }
                    let id = next_id(&generation, &mut serial);
                    if framing::write(&stdin, json!({"type":"clear_queue","id":id}))
                        .await
                        .is_err()
                    {
                        let _ = response.send(Err(closed_error()));
                        break 'protocol "Pi queue cancellation failed";
                    }
                    pending.insert(
                        id,
                        Pending::CancelClear {
                            turn_id: turn.id.clone(),
                            response,
                        },
                    );
                }
                Some(Command::Interaction {
                    request_id,
                    reply,
                    response,
                }) => {
                    let valid = dialogs.get(&request_id).filter(|dialog| {
                        active
                            .as_ref()
                            .is_some_and(|turn| !turn.cancelling && turn.id == dialog.turn_id)
                            && dialog
                                .expires
                                .is_none_or(|deadline| Instant::now() < deadline)
                    });
                    let result = match valid {
                        Some(dialog) => interactions::response(&dialog.record, &reply),
                        None => Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Pi interaction is stale or settled",
                        )),
                    };
                    let wire = match result {
                        Ok(wire) => wire,
                        Err(error) => {
                            let _ = response.send(Err(error));
                            continue;
                        }
                    };
                    dialogs.remove(&request_id);
                    if let Ok(mut visible) = permissions.lock() {
                        visible.remove(&request_id);
                    }
                    let written = framing::write(&stdin, wire).await;
                    let failed = written.is_err();
                    let _ = response.send(written);
                    if failed {
                        break 'protocol "Pi interaction response write failed";
                    }
                }
                Some(Command::Close) | None => break 'protocol "Pi RPC session was closed",
            },
            frame = frames.recv() => {
                let record = match frame {
                    Some(Ok(record)) => record,
                    _ => break 'protocol "Pi RPC lifecycle stream ended or became invalid",
                };
                if record["type"] == "response" {
                    let Some(id) = record["id"].as_str() else {
                        break 'protocol "Pi RPC returned an uncorrelated command response";
                    };
                    let Some(request) = pending.remove(id) else {
                        continue;
                    };
                    let success = record["success"] == true;
                    match request {
                        Pending::Request { command, response } => {
                            if record["command"] != command {
                                let _ = response.send(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "Pi command response identity changed",
                                )));
                                break 'protocol "Pi command response identity changed";
                            }
                            if success && command == "get_state" && record["data"]["sessionId"] != session_id {
                                let _ = response.send(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "Pi session identity changed",
                                )));
                                break 'protocol "Pi session identity changed";
                            }
                            let result = if success {
                                Ok(record["data"].clone())
                            } else {
                                Err(io::Error::other(format!("Pi RPC {command} was rejected")))
                            };
                            let _ = response.send(result);
                        }
                        Pending::Prompt { turn_id } => {
                            if record["command"] != "prompt" {
                                break 'protocol "Pi prompt response identity changed";
                            }
                            let Some(turn) = active.as_mut().filter(|turn| turn.id == turn_id) else {
                                continue;
                            };
                            if !success || record["data"]["disposition"] == "handled" {
                                if let Some(response) = turn.admission.take() {
                                    let _ = response.send(Err(io::Error::other(if success {
                                        "Pi handled the input without starting its controlled model task"
                                    } else {
                                        "Pi rejected the controlled prompt before execution"
                                    })));
                                }
                                if turn.admitted {
                                    turn.finish(
                                        &events,
                                        &generation,
                                        &session_id,
                                        if turn.cancelling {
                                            "interrupted"
                                        } else {
                                            "failed"
                                        },
                                    );
                                }
                                active = None;
                                dialogs.clear();
                                clear_permissions(&permissions);
                            } else if record["data"]["disposition"] == "queued" {
                                // No other provider turn can be associated with this
                                // prompt. Remove the queue before invalidating; never
                                // complete it at an unrelated agent_settled event.
                                if let Some(turn) = active.take()
                                    && let Some(admission) = turn.admission
                                {
                                    let _ = admission.send(Err(io::Error::other(
                                        "Pi unexpectedly queued the controlled prompt; it was not replayed",
                                    )));
                                }
                                dialogs.clear();
                                clear_permissions(&permissions);
                                let id = next_id(&generation, &mut serial);
                                if framing::write(&stdin, json!({"type":"clear_queue","id":id}))
                                    .await
                                    .is_err()
                                {
                                    break 'protocol "Pi unexpected queue could not be cleared";
                                }
                                pending.insert(id, Pending::RejectQueue);
                            } else if record["data"]["disposition"] != "started" {
                                break 'protocol "Pi returned an unsupported prompt disposition";
                            } else {
                                turn.provider_started = true;
                            }
                        }
                        Pending::CancelClear { turn_id, response } => {
                            if !success || record["command"] != "clear_queue" {
                                let _ = response.send(Err(io::Error::other(
                                    "Pi queue removal failed; cancellation was not confirmed",
                                )));
                                break 'protocol "Pi queue removal failed";
                            }
                            if active.as_ref().is_none_or(|turn| turn.id != turn_id) {
                                let _ = response.send(Ok(Some(turn_id)));
                                continue;
                            }
                            let provider_started = active.as_ref().is_some_and(|turn| turn.provider_started);
                            let id = next_id(&generation, &mut serial);
                            if framing::write(&stdin, json!({"type":"abort","id":id}))
                                .await
                                .is_err()
                            {
                                let _ = response.send(Err(closed_error()));
                                break 'protocol "Pi abort write failed";
                            }
                            pending.insert(
                                id,
                                Pending::CancelAbort {
                                    turn_id,
                                    provider_started,
                                    response,
                                },
                            );
                        }
                        Pending::CancelAbort {
                            turn_id,
                            provider_started,
                            response,
                        } => {
                            if !success || record["command"] != "abort" {
                                let _ = response.send(Err(io::Error::other("Pi abort was not confirmed")));
                                break 'protocol "Pi abort failed";
                            }
                            // Official Pi abort does not own outstanding prompt preflight.
                            // A late started event cannot prove an earlier idle abort cancelled it.
                            if !provider_started {
                                let _=response.send(Err(io::Error::new(io::ErrorKind::BrokenPipe,"Pi prompt preflight could not be cancelled safely; the managed session must stop")));
                                break 'protocol "Pi preflight cancellation requires stopping its owned process";
                            }
                            if active.as_ref().is_some_and(|turn| turn.id == turn_id)
                                && let Some(turn) = active.take()
                                && turn.admitted
                            {
                                turn.finish(&events, &generation, &session_id, "interrupted");
                            }
                            dialogs.clear();
                            clear_permissions(&permissions);
                            let _ = response.send(Ok(Some(turn_id)));
                        }
                        Pending::RejectQueue => {
                            break 'protocol "Pi queued a prompt without establishing its controlled owner; it was not replayed";
                        }
                    }
                    continue;
                }
                if interactions::dialog(&record) {
                    let token = format!("pi-interaction-{}", uuid::Uuid::new_v4().simple());
                    let metadata = match interactions::metadata(&record, &token) {
                        Ok(metadata) => metadata,
                        Err(_) => break 'protocol "Pi emitted an invalid interaction",
                    };
                    let Some(turn) = active.as_mut() else {
                        if framing::write(
                            &stdin,
                            json!({"type":"extension_ui_response","id":record["id"],"cancelled":true}),
                        )
                        .await
                        .is_err()
                        {
                            break 'protocol "Unowned Pi dialog could not be declined";
                        }
                        continue;
                    };
                    if turn.cancelling {
                        if framing::write(
                            &stdin,
                            json!({"type":"extension_ui_response","id":record["id"],"cancelled":true}),
                        )
                        .await
                        .is_err()
                        {
                            break 'protocol "Late Pi dialog cancellation failed";
                        }
                        continue;
                    }
                    if dialogs.len() >= 64 {
                        break 'protocol "Pi interaction capacity exceeded";
                    }
                    if let Some(previous) = dialogs
                        .values()
                        .find(|dialog| dialog.record["id"] == record["id"])
                    {
                        if previous.record == record {
                            continue;
                        }
                        break 'protocol "Pi reused an active interaction identity";
                    }
                    turn.admit(&events, &generation, &session_id);
                    let expires = record["timeout"].as_u64().and_then(|timeout| {
                        Instant::now().checked_add(std::time::Duration::from_millis(timeout))
                    });
                    dialogs.insert(
                        token.clone(),
                        Interaction {
                            record,
                            turn_id: turn.id.clone(),
                            expires,
                        },
                    );
                    if let Ok(mut visible) = permissions.lock() {
                        visible.insert(token.clone(), metadata);
                    }
                    events::publish(
                        &events,
                        &generation,
                        None,
                        json!({"method":"cccc/approvalRequired","params":{"threadId":session_id,"turnId":turn.id,"request_id":token}}),
                    );
                    continue;
                }
                if record["type"] == "agent_start" && active.is_none() {
                    break 'protocol "Pi started work without a current controlled owner";
                }
                if let Some(turn) = active.as_mut() {
                    match turn.observe(&record, &events, &generation, &session_id) {
                        Ok(true) => {
                            active = None;
                            dialogs.clear();
                            clear_permissions(&permissions);
                        }
                        Ok(false) => {}
                        Err(_) => break 'protocol "Pi controlled prompt lifecycle could not be correlated",
                    }
                }
            }
        }
    };
    running.store(false, Ordering::Release);
    clear_permissions(&permissions);
    if let Some(turn) = active
        && let Some(response) = turn.admission
    {
        let _ = response.send(Err(io::Error::new(io::ErrorKind::BrokenPipe, reason)));
    }
    for (_, request) in pending {
        match request {
            Pending::Request { response, .. } => {
                let _ = response.send(Err(closed_error()));
            }
            Pending::CancelClear { response, .. } | Pending::CancelAbort { response, .. } => {
                let _ = response.send(Err(closed_error()));
            }
            _ => {}
        }
    }
    events::publish(
        &events,
        &generation,
        None,
        json!({"method":MANAGED_AGENT_DISCONNECTED_METHOD,"params":{"threadId":session_id,"reason":reason}}),
    );
}

fn next_id(generation: &str, serial: &mut u64) -> String {
    *serial += 1;
    format!("cccc-pi-{generation}-{serial}")
}
fn clear_permissions(permissions: &Mutex<BTreeMap<String, Value>>) {
    if let Ok(mut visible) = permissions.lock() {
        visible.clear();
    }
}
