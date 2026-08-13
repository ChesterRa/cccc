use axum::extract::ws::{Message, WebSocket};
use cccc_core::{GroupStore, voice_recording_lease};
use serde_json::{Value, json};

use super::{
    assistant, load, send_voice_ws_error, voice_asr, voice_backend_access, voice_ws_capture,
    voice_ws_lifecycle,
};
use crate::AppState;

pub(super) async fn serve(
    state: AppState,
    group_id: String,
    owner_id: String,
    lease_id: String,
    recording_lease: Value,
    mut socket: WebSocket,
) {
    let group_title = GroupStore::new(state.home.clone())
        .and_then(|store| store.load(&group_id))
        .map(|group| group.title)
        .unwrap_or_else(|_| group_id.clone());
    let loaded = load(&state, &group_id);
    let assistant = match loaded
        .map(|value| assistant(&value))
        .and_then(|item| voice_backend_access::require_local_asr(&item).map(|_| item))
    {
        Ok(value) => value,
        Err(error) => {
            let _ = send_json(
                &mut socket,
                json!({"type":"error","ok":false,"error":{
                    "code":"assistant_unavailable","message":error.to_string()
                }}),
            )
            .await;
            release_lease(&state, &group_id, &owner_id, &lease_id);
            return;
        }
    };
    let mut capture = voice_ws_capture::VoiceWsCapture::new(&assistant);
    let mut stopped = false;
    let mut lease_released = false;
    let mut lease_renewed_at = std::time::Instant::now();
    let mut shutdown = state.shutdown.subscribe();

    loop {
        let message = tokio::select! {
            _ = shutdown.recv() => break,
            message = socket.recv() => message,
        };
        let Some(Ok(message)) = message else {
            break;
        };
        if matches!(message, Message::Close(_)) {
            break;
        }
        if let Message::Binary(bytes) = message {
            if !capture.is_started() {
                let error = voice_asr::VoiceError::new(
                    "audio_before_start",
                    "binary audio received before the start command",
                );
                let _ = send_voice_ws_error(&mut socket, &error).await;
                break;
            }
            if lease_renewed_at.elapsed() >= std::time::Duration::from_secs(5) {
                if !renew_lease(
                    &state,
                    &group_id,
                    &group_title,
                    &owner_id,
                    &lease_id,
                    &mut socket,
                )
                .await
                {
                    break;
                }
                lease_renewed_at = std::time::Instant::now();
            }
            match capture.accept_audio(&bytes).await {
                Ok(events) => {
                    if send_events(&mut socket, events).await.is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = send_voice_ws_error(&mut socket, &error).await;
                    break;
                }
            }
            continue;
        }
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(command) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        match command["type"].as_str().unwrap_or("") {
            "start" => {
                match voice_ws_lifecycle::validate_recording_lease_scope(&recording_lease, &command)
                {
                    Err(error) => {
                        let _ = send_voice_ws_error(&mut socket, &error).await;
                        break;
                    }
                    Ok(()) => match capture.start(&state.home, &command).await {
                        Ok(event) => {
                            if send_json(&mut socket, event).await.is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = send_voice_ws_error(&mut socket, &error).await;
                            break;
                        }
                    },
                }
            }
            "audio" => {
                let error = voice_asr::VoiceError::new(
                    "binary_audio_required",
                    "send PCM16 audio as binary WebSocket frames",
                );
                let _ = send_voice_ws_error(&mut socket, &error).await;
                break;
            }
            "stop" => match capture
                .stop(&state, &group_id, command["seq"].clone())
                .await
            {
                Ok(events) => {
                    let _ = send_events(&mut socket, events).await;
                    release_lease(&state, &group_id, &owner_id, &lease_id);
                    lease_released = true;
                    let _ = send_json(
                        &mut socket,
                        json!({"type":"closed","ok":true,"seq":command["seq"]}),
                    )
                    .await;
                    stopped = true;
                    break;
                }
                Err(error) => {
                    let _ = send_voice_ws_error(&mut socket, &error).await;
                    break;
                }
            },
            _ => {}
        }
    }
    if !stopped {
        capture
            .finalize_disconnect(state.clone(), group_id.clone())
            .await;
    }
    if !lease_released {
        release_lease(&state, &group_id, &owner_id, &lease_id);
    }
}

async fn renew_lease(
    state: &AppState,
    group_id: &str,
    group_title: &str,
    owner_id: &str,
    lease_id: &str,
    socket: &mut WebSocket,
) -> bool {
    match voice_recording_lease::renew(&state.home, group_id, group_title, owner_id, lease_id) {
        Ok(true) => true,
        Ok(false) => {
            tracing::warn!(%group_id, %owner_id, "voice transcription websocket lost its recording lease");
            let _ = send_json(socket, json!({"type":"error","ok":false,"error":{
                "code":"assistant_voice_recording_lease_lost",
                "message":"voice secretary recording lease is missing, expired, or owned by another client"
            }})).await;
            false
        }
        Err(error) => {
            tracing::warn!(%group_id, %owner_id, %error, "voice transcription websocket could not renew its recording lease; keeping the active stream");
            true
        }
    }
}

fn release_lease(state: &AppState, group_id: &str, owner_id: &str, lease_id: &str) {
    if let Err(error) = voice_recording_lease::release(&state.home, group_id, owner_id, lease_id) {
        tracing::warn!(%error, %group_id, %owner_id, "voice recording lease release failed");
    }
}

async fn send_events(socket: &mut WebSocket, events: Vec<Value>) -> Result<(), axum::Error> {
    for event in events {
        send_json(socket, event).await?;
    }
    Ok(())
}

async fn send_json(socket: &mut WebSocket, value: Value) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string().into())).await
}
