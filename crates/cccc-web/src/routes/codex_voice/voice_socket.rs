use axum::extract::ws::{Message, WebSocket};
use cccc_daemon::experimental_codex_voice::{
    AnalystLifecycleEvent, parse_provider_delegation, realtime_greeting_commands,
    realtime_notice_commands,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

use crate::AppState;

const MAX_BROWSER_EVENT_BYTES: usize = 128 * 1024;

pub(super) async fn serve(
    mut socket: WebSocket,
    state: AppState,
    attachment: crate::codex_voice::SessionAttachment,
) {
    let session = Arc::clone(attachment.session());
    let info = session.info();
    let generation = info.generation.clone();
    let call = Arc::clone(session.call());
    let mut lifecycle_events = call.analyst().subscribe_lifecycle();
    let mut shutdown = state.shutdown.subscribe();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    if !send_json(
        &mut socket,
        json!({"type":"ready","call":super::payload::info_value(info.clone())}),
    )
    .await
    {
        finish(&state, attachment).await;
        return;
    }
    for command in realtime_greeting_commands(true) {
        if !send_provider_command(&mut socket, command).await {
            finish(&state, attachment).await;
            return;
        }
    }

    'session: loop {
        tokio::select! {
            _ = shutdown.recv() => break,
            _ = heartbeat.tick() => {
                if let Err(error) = call.heartbeat(&generation) {
                    tracing::warn!(%error, "Codex Voice recording lease heartbeat failed");
                    let _ = send_error(&mut socket, "recording_lease_lost", "The Codex Voice recording lease was lost.").await;
                    break;
                }
                // Keep proxy and browser stacks from reclaiming an otherwise idle connection.
                if !send_json(&mut socket, json!({"type":"heartbeat"})).await { break; }
            }
            browser = socket.recv() => {
                let Some(Ok(browser)) = browser else { break; };
                let text = match browser {
                    Message::Text(text) => text,
                    Message::Close(_) => break,
                    _ => continue,
                };
                if text.len() > MAX_BROWSER_EVENT_BYTES {
                    let _ = send_error(&mut socket, "event_too_large", "Codex Voice browser event is oversized.").await;
                    break;
                }
                let value: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(_) => {
                        if !send_error(&mut socket, "invalid_event", "Codex Voice browser event must be JSON.").await { break; }
                        continue;
                    }
                };
                match value["type"].as_str().unwrap_or_default() {
                    "provider_event" => {
                        let provider_event = &value["event"];
                        let provider = match parse_provider_delegation(provider_event) {
                            Ok(provider) => provider,
                            Err(error) => {
                                tracing::warn!(%error, "invalid Codex Realtime delegation event");
                                if !send_error(&mut socket, "invalid_provider_event", "Codex Voice received an invalid delegation event.").await { break; }
                                continue;
                            }
                        };
                        if provider.is_none() { continue; }
                        match call.begin_provider_event(&generation, provider_event).await {
                            Ok(Some(_)) | Ok(None) => {}
                            Err(error) => {
                                for command in realtime_notice_commands(
                                    "I couldn't start that investigation. Please check the Voice Analyst status in CCCC.",
                                ) {
                                    if !send_provider_command(&mut socket, command).await { break 'session; }
                                }
                                tracing::warn!(%error, "Voice Analyst investigation start failed");
                                if !send_error(&mut socket, "analyst_start_failed", "The Voice Analyst could not start that investigation.").await { break; }
                            }
                        }
                    }
                    "cancel_current" | "cancel" => match call.cancel_current(&generation).await {
                        Ok(true) => {
                            if !send_json(&mut socket, json!({"type":"analyst_cancelling"})).await { break; }
                        }
                        Ok(false) => {
                            if !send_error(&mut socket, "analyst_not_working", "The Voice Analyst has no active investigation.").await { break; }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "Voice Analyst cancellation failed");
                            if !send_error(&mut socket, "cancel_failed", "The Voice Analyst could not cancel the current investigation.").await { break; }
                        }
                    },
                    "heartbeat" => {
                        if !send_json(&mut socket, json!({"type":"heartbeat"})).await { break; }
                    }
                    "stop" => break,
                    _ => {
                        if !send_error(&mut socket, "invalid_event", "Unknown Codex Voice browser event.").await { break; }
                    }
                }
            }
            lifecycle = lifecycle_events.recv() => match lifecycle {
                Ok(AnalystLifecycleEvent::Started { receipt, origin }) => {
                    if origin.is_actor_result() && origin.speakable() {
                        call.follow_analyst_turn(&receipt).await;
                    }
                    if !send_json(&mut socket, json!({"type":"analyst_working"})).await { break; }
                }
                Ok(AnalystLifecycleEvent::Progress { turn_id, text, speakable }) => {
                    if speakable {
                        if !send_json(&mut socket, json!({"type":"analyst_progress","text":text})).await { break; }
                        match call.project_analyst_delta(&generation, &turn_id, &text).await {
                            Ok(commands) => for command in commands {
                                if !send_provider_command(&mut socket, command).await { break 'session; }
                            },
                            Err(error) => {
                                tracing::warn!(%error, "Voice Analyst progress projection failed");
                                let _ = send_error(&mut socket, "analyst_projection_failed", "The Voice Analyst result could not be returned to Realtime Voice.").await;
                                break;
                            }
                        }
                    }
                }
                Ok(AnalystLifecycleEvent::Completed { turn_id, delegation_id, status, result, speakable }) => {
                    if status == "completed" && !result.trim().is_empty() {
                        if speakable {
                            match call.take_final_projection(
                                &generation, &delegation_id, &turn_id, &result,
                            ).await {
                                Ok(Some(projection)) => for command in projection.commands {
                                    if !send_provider_command(&mut socket, command).await { break 'session; }
                                },
                                Ok(None) => {}
                                Err(error) => {
                                    tracing::warn!(%error, "Voice Analyst final projection failed");
                                    let _ = send_error(&mut socket, "analyst_projection_failed", "The Voice Analyst result could not be returned to Realtime Voice.").await;
                                    break;
                                }
                            }
                        }
                        if !send_json(&mut socket, json!({"type":"analyst_result","text":result})).await { break; }
                    } else {
                        let _ = call.settle_without_projection(&generation, &turn_id).await;
                        if speakable {
                            for command in realtime_notice_commands(
                                "The investigation did not complete. Please check the Voice Analyst status in CCCC.",
                            ) {
                                if !send_provider_command(&mut socket, command).await { break 'session; }
                            }
                        }
                    }
                    if !send_json(&mut socket, json!({"type":"analyst_terminal","status":status})).await { break; }
                }
                Ok(AnalystLifecycleEvent::NeedsAttention { code }) => {
                    let _ = send_error(
                        &mut socket, code,
                        "The Voice Analyst encountered an incompatible approval or protocol request.",
                    ).await;
                    break;
                }
                Ok(AnalystLifecycleEvent::Disconnected) => {
                    let _ = send_error(&mut socket, "analyst_disconnected", "The Voice Analyst disconnected.").await;
                    break;
                }
                Ok(AnalystLifecycleEvent::TrackedWork(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => tracing::warn!(
                    skipped,
                    "Codex Voice WebSocket fell behind Analyst progress; continuing from the retained event tail"
                ),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
    finish(&state, attachment).await;
}

async fn finish(state: &AppState, attachment: crate::codex_voice::SessionAttachment) {
    let info = attachment.session().info();
    if let Err(error) = state
        .codex_voice
        .stop(&info.group_id, &info.generation)
        .await
    {
        tracing::warn!(%error, group_id = %info.group_id, generation = %info.generation, "Codex Voice connection cleanup failed");
    }
    drop(attachment);
}

async fn send_provider_command(socket: &mut WebSocket, command: Value) -> bool {
    send_json(socket, json!({"type":"provider_command","message":command})).await
}

async fn send_error(socket: &mut WebSocket, code: &str, message: &str) -> bool {
    send_json(
        socket,
        json!({"type":"error","code":code,"message":message}),
    )
    .await
}

async fn send_json(socket: &mut WebSocket, value: Value) -> bool {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}
