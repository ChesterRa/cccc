use axum::extract::ws::{Message, WebSocket};
use cccc_daemon::experimental_codex_voice::{
    AnalystLifecycleEvent, CodexVoiceCall, VoiceDelegationAdmission, parse_provider_delegation,
    realtime_greeting_commands, realtime_notice_commands,
};
use serde_json::{Value, json};
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::AppState;

const MAX_BROWSER_EVENT_BYTES: usize = 128 * 1024;
const RECORDING_LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

struct RecordingLeaseHeartbeat {
    failure: oneshot::Receiver<String>,
    task: JoinHandle<()>,
}

impl RecordingLeaseHeartbeat {
    fn start(call: Arc<CodexVoiceCall>, generation: String) -> Self {
        Self::start_with(RECORDING_LEASE_HEARTBEAT_INTERVAL, move || {
            call.heartbeat(&generation)
        })
    }

    fn start_with<F, E>(interval: Duration, mut heartbeat: F) -> Self
    where
        F: FnMut() -> Result<(), E> + Send + 'static,
        E: Display + Send + 'static,
    {
        let (failure_sender, failure) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                if let Err(error) = heartbeat() {
                    let _ = failure_sender.send(error.to_string());
                    return;
                }
            }
        });
        Self { failure, task }
    }

    async fn failed(&mut self) -> String {
        (&mut self.failure)
            .await
            .unwrap_or_else(|_| "recording lease heartbeat task stopped unexpectedly".to_owned())
    }

    async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}

impl Drop for RecordingLeaseHeartbeat {
    fn drop(&mut self) {
        self.task.abort();
    }
}

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
    // Analyst admission can outlast the recording TTL, so renewal must not share its event loop.
    let mut lease_heartbeat = RecordingLeaseHeartbeat::start(Arc::clone(&call), generation.clone());
    let mut socket_heartbeat = tokio::time::interval(RECORDING_LEASE_HEARTBEAT_INTERVAL);
    socket_heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    if !send_json(
        &mut socket,
        json!({"type":"ready","call":super::payload::info_value(info.clone())}),
    )
    .await
    {
        lease_heartbeat.stop().await;
        finish(&state, attachment).await;
        return;
    }
    for command in realtime_greeting_commands() {
        if !send_provider_command(&mut socket, command).await {
            lease_heartbeat.stop().await;
            finish(&state, attachment).await;
            return;
        }
    }

    'session: loop {
        tokio::select! {
            _ = shutdown.recv() => break,
            error = lease_heartbeat.failed() => {
                tracing::warn!(%error, "Codex Voice recording lease heartbeat failed");
                let _ = send_error(&mut socket, "recording_lease_lost", "The Codex Voice recording lease was lost.").await;
                break;
            }
            _ = socket_heartbeat.tick() => {
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
                    "provider_error" => {
                        log_provider_error(&generation, &value["error"]);
                    }
                    "provider_receipt" => {
                        // Browser transport diagnostics only: these counters neither
                        // authorize Analyst work nor prove that every fact was spoken.
                        tracing::info!(
                            %generation,
                            sent = value["sent"].as_u64(),
                            acknowledged = value["acknowledged"].as_u64(),
                            pending = value["pending"].as_u64(),
                            speech_turns_completed = value["speech_turns_completed"].as_u64(),
                            "Codex Voice provider delivery receipt"
                        );
                    }
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
                        match call.route_provider_event(&generation, provider_event).await {
                            Ok(Some(VoiceDelegationAdmission::NativeInput { delegation_id, text })) => {
                                let delivery = session.analyst().submit_native_voice_input(&text).await;
                                if !matches!(delivery, Ok(true)) {
                                    let rolled_back = call
                                        .reject_native_delegation(&generation, &delegation_id)
                                        .await
                                        .unwrap_or(true);
                                    // A failing terminal write can race the Runtime's authoritative
                                    // input echo. Once correlated, the delegation was accepted and
                                    // must neither be rejected nor replayed.
                                    if !rolled_back {
                                        continue;
                                    }
                                    let error = match delivery {
                                        Ok(false) => "the native Runtime terminal rejected the input".to_owned(),
                                        Err(error) => error.to_string(),
                                        Ok(true) => unreachable!(),
                                    };
                                    tracing::warn!(%error, %delegation_id, "Voice Analyst native input delivery failed");
                                    for command in realtime_notice_commands(
                                        "I couldn't deliver that investigation to the Voice Analyst. Please check its terminal in CCCC.",
                                    ) {
                                        if !send_provider_command(&mut socket, command).await { break 'session; }
                                    }
                                    if !send_error(
                                        &mut socket,
                                        "analyst_delivery_failed",
                                        "The Voice Analyst did not accept that investigation.",
                                    ).await { break; }
                                }
                            }
                            Ok(Some(VoiceDelegationAdmission::Turn(_)
                                | VoiceDelegationAdmission::NativeInputPending))
                            | Ok(None) => {}
                            Err(error) => {
                                for command in realtime_notice_commands(
                                    "I couldn't deliver that investigation to the Voice Analyst. Please check its terminal in CCCC.",
                                ) {
                                    if !send_provider_command(&mut socket, command).await { break 'session; }
                                }
                                tracing::warn!(%error, "Voice Analyst investigation delivery failed");
                                if !send_error(
                                    &mut socket,
                                    "analyst_delivery_failed",
                                    "The Voice Analyst did not accept that investigation.",
                                ).await { break; }
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
                    if origin.speakable() {
                        call.follow_analyst_turn(&receipt).await;
                    }
                    if !send_json(&mut socket, json!({"type":"analyst_working"})).await { break; }
                }
                Ok(AnalystLifecycleEvent::Associated { receipt, origin }) => {
                    if origin.speakable() {
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
                        if status == "result_too_large" && !send_error(
                            &mut socket,
                            "analyst_result_too_large",
                            "The Analyst result is too long for Voice. Read the full result in its terminal or ask for a shorter summary.",
                        ).await { break; }
                        if speakable {
                            let notice = if status == "result_too_large" {
                                "The Analyst finished, but the result is too long for Voice. Do not present earlier partial updates as the complete result. The full output is in the Analyst terminal; the user can ask for a shorter summary."
                            } else {
                                "The investigation did not complete. Please check the Voice Analyst status in CCCC."
                            };
                            for command in realtime_notice_commands(notice) {
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
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "Codex Voice WebSocket fell behind Analyst lifecycle; closing the unreplayable stream"
                    );
                    let _ = send_error(
                        &mut socket,
                        "analyst_event_gap",
                        "The Voice Analyst event stream lost state and must reconnect.",
                    ).await;
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
    lease_heartbeat.stop().await;
    finish(&state, attachment).await;
}

async fn finish(state: &AppState, attachment: crate::codex_voice::SessionAttachment) {
    let info = attachment.session().info();
    if let Err(error) = state.codex_voice.stop(&info.generation).await {
        tracing::warn!(%error, generation = %info.generation, "Codex Voice connection cleanup failed");
    }
    drop(attachment);
}

async fn send_provider_command(socket: &mut WebSocket, command: Value) -> bool {
    send_json(socket, json!({"type":"provider_command","message":command})).await
}

fn log_provider_error(generation: &str, error: &Value) {
    // Browser-reported transport diagnostics, not Analyst lifecycle authority.
    // Never log the provider explanation: it can quote user input or secrets.
    tracing::warn!(
        %generation,
        provider_code = provider_error_identifier(&error["code"]),
        provider_error_type = provider_error_identifier(&error["type"]),
        provider_event_id = provider_error_identifier(&error["event_id"]),
        provider_param = provider_error_identifier(&error["param"]),
        "Codex Realtime Voice provider error"
    );
}

fn provider_error_identifier(value: &Value) -> &str {
    let text = value.as_str().unwrap_or_default().trim();
    if !text.is_empty()
        && text.len() <= 128
        && text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'[' | b']' | b'-')
        })
    {
        text
    } else {
        ""
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    #[test]
    fn provider_error_log_has_call_identity_but_no_explanation_or_extra_payload() {
        let mut output = tempfile::tempfile().expect("diagnostic capture");
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(output.try_clone().expect("capture writer"))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            log_provider_error(
                "voice-call-1",
                &json!({
                    "code":"rate_limit_exceeded",
                    "type":"invalid_request_error",
                    "event_id":"event-1",
                    "message":"private provider explanation",
                    "authorization":"private-token"
                }),
            );
        });
        output.rewind().expect("rewind diagnostic");
        let mut text = String::new();
        output.read_to_string(&mut text).expect("read diagnostic");
        for expected in [
            "voice-call-1",
            "rate_limit_exceeded",
            "invalid_request_error",
            "event-1",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(!text.contains("private"));
    }

    #[test]
    fn provider_error_identifiers_are_bounded_and_cannot_inject_log_lines() {
        for text in [
            "rate_limit_exceeded",
            "HTTP:429",
            "content[0].text",
            "event-ab_cd",
        ] {
            let value = json!(text);
            assert_eq!(provider_error_identifier(&value), text);
        }
        for value in [
            Value::Null,
            json!({"code":"nested"}),
            json!("x".repeat(129)),
            json!("bad\ninjected"),
            json!("Bearer private-value"),
            json!("https://example.com/?token=private-value"),
        ] {
            assert_eq!(provider_error_identifier(&value), "");
        }
    }

    #[tokio::test]
    async fn recording_lease_heartbeat_keeps_running_while_socket_work_waits() {
        let ticks = Arc::new(AtomicUsize::new(0));
        let reached_three_ticks = Arc::new(Notify::new());
        let observed_ticks = Arc::clone(&ticks);
        let observed_notification = Arc::clone(&reached_three_ticks);
        let heartbeat = RecordingLeaseHeartbeat::start_with(
            Duration::from_millis(5),
            move || -> Result<(), &'static str> {
                if observed_ticks.fetch_add(1, Ordering::SeqCst) + 1 == 3 {
                    observed_notification.notify_one();
                }
                Ok(())
            },
        );

        tokio::time::timeout(Duration::from_secs(1), reached_three_ticks.notified())
            .await
            .expect("independent heartbeat must continue during long socket work");
        assert!(ticks.load(Ordering::SeqCst) >= 3);
        heartbeat.stop().await;
    }

    #[tokio::test]
    async fn recording_lease_heartbeat_reports_real_lease_loss() {
        let mut heartbeat = RecordingLeaseHeartbeat::start_with(
            Duration::from_millis(1),
            || -> Result<(), &'static str> { Err("lease lost") },
        );

        let failure = tokio::time::timeout(Duration::from_secs(1), heartbeat.failed())
            .await
            .expect("lease failure must be reported promptly");
        assert_eq!(failure, "lease lost");
        heartbeat.stop().await;
    }
}
