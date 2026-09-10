use super::{AsrError, active::Active, connection, persistence};
use crate::{
    AppState,
    routes::assistants::{voice_ws, voice_ws_lifecycle},
};
use axum::extract::ws::{CloseFrame, Message, WebSocket, close_code};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

enum Incoming {
    Browser(Option<Result<Message, axum::Error>>),
    Provider(
        Option<
            Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>,
        >,
    ),
    Shutdown,
    Deadline,
    Checkpoint,
}

pub(in crate::routes::assistants) async fn serve(
    state: AppState,
    group: String,
    owner: String,
    lease_id: String,
    lease: Value,
    assistant: Value,
    mut socket: WebSocket,
) {
    let _lease_guard = super::lease::LeaseGuard {
        home: state.home.clone(),
        group: group.clone(),
        owner: owner.clone(),
        lease_id: lease_id.clone(),
    };
    let mut browser_disconnected = false;
    let group_title = lease["group_title"].as_str().unwrap_or(&group).to_owned();
    let mut active: Option<Active> = None;
    let mut shutdown = state.shutdown.subscribe();
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(5));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut deadline: Option<tokio::time::Instant> = None;
    let mut failure: Option<AsrError> = None;
    let mut final_sent = false;
    let mut checkpoint_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    checkpoint_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let incoming = tokio::select! {
            _ = shutdown.recv() => Incoming::Shutdown,
            _ = checkpoint_tick.tick() => Incoming::Checkpoint,
            _ = heartbeat.tick() => {
                if !voice_ws::renew_lease(&state,&group,&group_title,&owner,&lease_id,&mut socket).await { break; }
                continue;
            },
            _ = async { match deadline { Some(time) => tokio::time::sleep_until(time).await, None => std::future::pending().await } } => Incoming::Deadline,
            message = socket.recv() => Incoming::Browser(message),
            message = async { match active.as_mut() { Some(run) => run.reader.next().await, None => std::future::pending().await } } => Incoming::Provider(message),
        };
        let result = match incoming {
            Incoming::Shutdown => break,
            Incoming::Checkpoint => match active.as_mut() {
                Some(run) => persistence::checkpoints(&state, &group, run, false).await,
                None => Ok(()),
            },
            Incoming::Browser(None | Some(Err(_)) | Some(Ok(Message::Close(_)))) => {
                browser_disconnected = true;
                break;
            }
            Incoming::Deadline => Err(AsrError::new(
                "external_asr_finish_timeout",
                "The ASR provider did not finish the recording in time",
            )),
            Incoming::Browser(Some(Ok(Message::Binary(bytes)))) => match active.as_mut() {
                Some(run) => match run.audio(&bytes).await {
                    Ok(events) => voice_ws::send_events(&mut socket, events)
                        .await
                        .map_err(|_| connection::transport_error()),
                    Err(error) => Err(error),
                },
                None => Err(AsrError::new(
                    "audio_before_start",
                    "Binary audio received before start",
                )),
            },
            Incoming::Browser(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<Value>(&text) {
                    Ok(command) => {
                        command_event(
                            &state,
                            &assistant,
                            &lease,
                            &mut active,
                            &mut socket,
                            &mut deadline,
                            command,
                        )
                        .await
                    }
                    Err(_) => Err(AsrError::new(
                        "invalid_command",
                        "Invalid recording command",
                    )),
                }
            }
            Incoming::Provider(Some(Ok(message))) => {
                let run = active
                    .as_mut()
                    .expect("provider events require an active session");
                match run.receive(message) {
                    Ok(events) => {
                        match persistence::checkpoints(&state, &group, run, false).await {
                            Ok(()) => voice_ws::send_events(&mut socket, events)
                                .await
                                .map_err(|_| connection::transport_error()),
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            Incoming::Provider(_) => Err(connection::transport_error()),
            _ => Ok(()),
        };
        if let Err(error) = result {
            failure = Some(error);
            break;
        }
        if active.as_ref().is_some_and(|run| run.completed) {
            let run = active.as_ref().expect("completed active session");
            let event = persistence::final_event(&state, &group, run).await;
            final_sent = voice_ws::send_json(&mut socket, event).await.is_ok();
            let _ = voice_ws::send_json(
                &mut socket,
                json!({"type":"closed","ok":true,"seq":run.stop_seq}),
            )
            .await;
            let _ = socket
                .send(Message::Close(Some(CloseFrame {
                    code: close_code::NORMAL,
                    reason: "".into(),
                })))
                .await;
            break;
        }
    }
    // Upstream failures preserve completed provider sentences in the same
    // durable transcript. Non-document modes recover text to the composer.
    if !final_sent && let Some(run) = active.as_mut() {
        if browser_disconnected {
            super::disconnect::finalize(&state, &group, run).await;
        } else {
            let event = persistence::recover(&state, &group, run).await;
            if run.persist {
                let _ = voice_ws::send_json(&mut socket, event).await;
            }
        }
    }
    if let Some(error) = failure {
        let recovered = active
            .as_ref()
            .filter(|run| !run.persist)
            .map(|run| run.transcript.text())
            .unwrap_or_default();
        let _ = voice_ws::send_json(
            &mut socket,
            json!({"type":"error","ok":false,"recovered_text":recovered,
            "error":{"code":error.code,"message":error.message}}),
        )
        .await;
    }
    if let Some(run) = active.as_mut() {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), run.writer.close()).await;
    }
    drop(active); // Closing transport and recording files cancels every remaining resource.
}

async fn command_event(
    state: &AppState,
    assistant: &Value,
    lease: &Value,
    active: &mut Option<Active>,
    socket: &mut WebSocket,
    deadline: &mut Option<tokio::time::Instant>,
    command: Value,
) -> Result<(), AsrError> {
    match command["type"].as_str().unwrap_or_default() {
        "start" => {
            if active.is_some() {
                return Err(AsrError::new(
                    "recording_already_started",
                    "Recording already started",
                ));
            }
            voice_ws_lifecycle::validate_recording_lease_scope(lease, &command)?;
            let run = Active::start(state, assistant, &command).await?;
            let ready = run.ready(command["seq"].clone());
            *active = Some(run);
            voice_ws::send_json(socket, ready)
                .await
                .map_err(|_| connection::transport_error())
        }
        "stop" => {
            let run = active
                .as_mut()
                .ok_or_else(|| AsrError::new("audio_before_start", "Recording has not started"))?;
            run.stop(command["seq"].clone()).await?;
            *deadline = Some(tokio::time::Instant::now() + connection::FINISH_TIMEOUT);
            Ok(())
        }
        "audio" => Err(AsrError::new(
            "binary_audio_required",
            "Send PCM16 audio as binary WebSocket frames",
        )),
        _ => Err(AsrError::new(
            "invalid_command",
            "Unknown recording command",
        )),
    }
}
