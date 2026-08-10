use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::DaemonRequest;
use cccc_core::GroupStore;
use cccc_core::integration_state;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::io;
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiError, ApiResult, call, object, success};

mod voice_asr;
mod voice_audio_upload;
mod voice_backend_access;
mod voice_diarization;
mod voice_final_asr;
mod voice_inference;
mod voice_pcm_recording;
mod voice_recording_lease;
mod voice_runtime;
mod voice_session;
mod voice_speaker_transcript;
mod voice_ws_lifecycle;

const STORE_KEY: &str = "assistants";

#[derive(Debug, Default, Deserialize)]
struct DocumentQuery {
    #[serde(default)]
    document_path: String,
    #[serde(default)]
    include_archived: bool,
}

#[derive(Debug, Default, Deserialize)]
struct TranscriptionWsQuery {
    #[serde(default)]
    owner_id: String,
    #[serde(default)]
    lease_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct TranscriptionQuery {
    #[serde(default)]
    language: String,
    #[serde(default)]
    by: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/assistants", get(list))
        .route(
            "/api/v1/groups/{group_id}/assistants/{assistant_id}",
            get(show),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/{assistant_id}/settings",
            axum::routing::put(update_settings),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/{assistant_id}/status",
            post(update_status),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions",
            post(transcribe).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/transcriptions/ws",
            get(transcription_ws),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/recording_lease",
            post(recording_lease),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/models/install",
            post(model_install),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/models/remove",
            post(model_remove).delete(model_remove),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/runtime/install",
            post(voice_runtime::install),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/runtime/remove",
            post(voice_runtime::remove),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/sessions/latest",
            get(latest_session),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/sessions/latest/transcript",
            axum::routing::delete(clear_latest_transcript),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/sessions/{session_id}",
            get(session),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/transcript_segments",
            post(transcript_segment),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/documents",
            get(documents).put(document_save),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/documents/select",
            post(document_select),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/documents/instructions",
            post(document_instruction),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/documents/archive",
            post(document_archive),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/inputs",
            post(input),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/prompt_drafts/ack",
            post(prompt_ack),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/ask_requests/clear",
            post(clear_asks),
        )
}

async fn list(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    let runtime_assistant = runtime_assistant(&state, &group_id).await;
    Ok(success(payload(
        &state,
        &group_id,
        &load(&state, &group_id)?,
        runtime_assistant.as_ref(),
    )))
}
async fn show(
    State(state): State<AppState>,
    Path((group_id, assistant_id)): Path<(String, String)>,
) -> ApiResult {
    if assistant_id != "voice_secretary" {
        return Err(ApiError::not_found("assistant not found"));
    }
    let runtime_assistant = runtime_assistant(&state, &group_id).await;
    Ok(success(payload(
        &state,
        &group_id,
        &load(&state, &group_id)?,
        runtime_assistant.as_ref(),
    )))
}
async fn update_settings(
    State(state): State<AppState>,
    Path((group_id, assistant_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_assistant(&assistant_id)?;
    call(&state,"assistant_settings_update",object(json!({"group_id":group_id,"assistant_id":assistant_id,"by":body["by"].as_str().unwrap_or("user"),"patch":body}))).await
}
async fn update_status(
    State(state): State<AppState>,
    Path((group_id, assistant_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_assistant(&assistant_id)?;
    call(&state,"assistant_status_update",object(json!({"group_id":group_id,"assistant_id":assistant_id,"by":body["by"].as_str().unwrap_or("user"),"lifecycle":body["lifecycle"],"health":body["health"]}))).await
}
async fn transcribe(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<TranscriptionQuery>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult {
    voice_audio_upload::validate_content_length(&headers)?;
    let current = load(&state, &group_id)?;
    let assistant = assistant(&current);
    voice_backend_access::require_local_asr(&assistant)?;
    let selected = assistant["config"]["service_model_id"]
        .as_str()
        .unwrap_or("");
    let mime_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let language = query.language.trim().to_owned();
    let audio_file = voice_audio_upload::receive(&state.home, body).await?;
    let permit = voice_final_asr::try_acquire().ok_or_else(|| {
        ApiError::unavailable("asr_busy", "final ASR is busy with another recording")
    })?;
    let home = state.home.clone();
    let selected = selected.to_owned();
    let result = voice_final_asr::transcribe_file(
        permit,
        home,
        selected,
        language.clone(),
        audio_file,
        mime_type.clone(),
    )
    .await
    .map_err(|error| ApiError::unavailable("asr_task_failed", error.to_string()))?
    .map_err(voice_error)?;
    Ok(success(json!({
        "group_id":group_id,"assistant":assistant,"transcript":result["text"],
        "mime_type":mime_type,"language":language,"by":if query.by.trim().is_empty(){"user"}else{query.by.trim()},"bytes":result["bytes"],
        "backend":"assistant_service_local_asr","service":voice_asr::runtime_status(),
        "asr":{"available":true,"model_id":result["model_id"],"sample_rate":result["sample_rate"],"implementation":"rust"}
    })))
}

async fn transcription_ws(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<TranscriptionWsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    voice_recording_lease::validate(
        &state.home,
        &group_id,
        query.owner_id.trim(),
        query.lease_id.trim(),
    )?;
    Ok(ws.on_upgrade(move |socket| {
        serve_transcription_ws(state, group_id, query.owner_id, query.lease_id, socket)
    }))
}

async fn serve_transcription_ws(
    state: AppState,
    group_id: String,
    owner_id: String,
    lease_id: String,
    mut socket: WebSocket,
) {
    let loaded = load(&state, &group_id);
    let assistant = match loaded
        .map(|value| assistant(&value))
        .and_then(|item| voice_backend_access::require_local_asr(&item).map(|_| item))
    {
        Ok(value) => value,
        Err(error) => {
            let _=socket.send(Message::Text(json!({"type":"error","ok":false,"error":{"code":"assistant_unavailable","message":error.to_string()}}).to_string().into())).await;
            return;
        }
    };
    let selected = assistant["config"]["service_model_id"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let diarization_model = assistant["config"]["service_diarization_model_id"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let mut session: Option<voice_asr::StreamingSession> = None;
    let mut client_session_id = String::new();
    let mut document_path = String::new();
    let mut language = String::new();
    let mut persist_secretary_transcript = true;
    let mut recording: Option<voice_pcm_recording::PcmRecording> = None;
    let mut stopped = false;
    let mut audio_seq = 0_u64;
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
            if session.is_none() {
                let error = voice_asr::VoiceError {
                    code: "audio_before_start",
                    message: "binary audio received before the start command".into(),
                    details: Map::new(),
                };
                let _ = send_voice_ws_error(&mut socket, &error).await;
                break;
            }
            if lease_renewed_at.elapsed() >= std::time::Duration::from_secs(5) {
                match voice_recording_lease::renew(&state.home, &group_id, &owner_id, &lease_id) {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::warn!(
                            %group_id,
                            %owner_id,
                            "voice transcription websocket lost its recording lease"
                        );
                        let _=socket.send(Message::Text(json!({"type":"error","ok":false,"error":{"code":"assistant_voice_recording_lease_lost","message":"voice secretary recording lease is missing, expired, or owned by another client"}}).to_string().into())).await;
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(
                            %group_id,
                            %owner_id,
                            %error,
                            "voice transcription websocket could not renew its recording lease; keeping the active stream"
                        );
                    }
                }
                lease_renewed_at = std::time::Instant::now();
            }
            let Some(active_recording) = recording.as_mut() else {
                let error = voice_asr::VoiceError {
                    code: "audio_before_start",
                    message: "recording storage is not initialized".into(),
                    details: Map::new(),
                };
                let _ = send_voice_ws_error(&mut socket, &error).await;
                break;
            };
            if let Err(error) = active_recording.append(&bytes).await {
                let _ = send_voice_ws_error(&mut socket, &error).await;
                break;
            }
            let Some(mut active) = session.take() else {
                continue;
            };
            let decoded = tokio::task::spawn_blocking(move || {
                let result = active.accept_pcm16(16_000, &bytes);
                (active, result)
            })
            .await;
            let (active, result) = match decoded {
                Ok(value) => value,
                Err(error) => {
                    let wrapped = voice_asr::VoiceError {
                        code: "asr_task_failed",
                        message: error.to_string(),
                        details: Map::new(),
                    };
                    let _ = send_voice_ws_error(&mut socket, &wrapped).await;
                    break;
                }
            };
            session = Some(active);
            audio_seq = audio_seq.saturating_add(1);
            match result {
                Ok(Some(mut event)) => {
                    event["seq"] = json!(audio_seq);
                    let _ = socket.send(Message::Text(event.to_string().into())).await;
                }
                Ok(None) => {}
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
        if command["type"] == "start" {
            if session.is_some() || recording.is_some() {
                let error = voice_asr::VoiceError {
                    code: "recording_already_started",
                    message: "the active recording must be stopped before starting another one"
                        .into(),
                    details: Map::new(),
                };
                let _ = send_voice_ws_error(&mut socket, &error).await;
                break;
            }
            if command["sample_rate"].as_i64().unwrap_or(16_000) != 16_000 {
                let error = voice_asr::VoiceError {
                    code: "unsupported_sample_rate",
                    message: "streaming ASR requires 16000 Hz PCM16".into(),
                    details: Map::new(),
                };
                let _ = send_voice_ws_error(&mut socket, &error).await;
                break;
            }
            client_session_id = command["session_id"].as_str().unwrap_or("").to_owned();
            document_path = command["document_path"].as_str().unwrap_or("").to_owned();
            language = command["language"].as_str().unwrap_or("").to_owned();
            persist_secretary_transcript =
                voice_ws_lifecycle::persists_secretary_artifacts(&command);
            let home = state.home.clone();
            let selected = selected.clone();
            match tokio::task::spawn_blocking(move || {
                voice_asr::StreamingSession::open(&home, &selected)
            })
            .await
            {
                Err(error) => {
                    let wrapped = voice_asr::VoiceError {
                        code: "asr_task_failed",
                        message: error.to_string(),
                        details: Map::new(),
                    };
                    let _ = send_voice_ws_error(&mut socket, &wrapped).await;
                    break;
                }
                Ok(Err(error)) => {
                    let _ = send_voice_ws_error(&mut socket, &error).await;
                    break;
                }
                Ok(Ok(opened)) => {
                    let opened_recording =
                        match voice_pcm_recording::PcmRecording::create(&state.home) {
                            Ok(value) => value,
                            Err(error) => {
                                let _ = send_voice_ws_error(&mut socket, &error).await;
                                break;
                            }
                        };
                    let model_id = opened.model_id.clone();
                    session = Some(opened);
                    recording = Some(opened_recording);
                    let _=socket.send(Message::Text(json!({"type":"ready","ok":true,"seq":command["seq"],"sample_rate":16000,"audio_transport":"binary_pcm16","model_id":model_id,"backend":"assistant_service_local_asr"}).to_string().into())).await;
                }
            }
        } else if command["type"] == "audio" {
            let error = voice_asr::VoiceError {
                code: "binary_audio_required",
                message: "send PCM16 audio as binary WebSocket frames".into(),
                details: Map::new(),
            };
            let _ = send_voice_ws_error(&mut socket, &error).await;
            break;
        } else if command["type"] == "stop" {
            if let Some(mut active) = session.take() {
                match tokio::task::spawn_blocking(move || {
                    let event = active.finish();
                    (active, event)
                })
                .await
                {
                    Ok((active, event)) => {
                        session = Some(active);
                        if let Some(mut event) = event {
                            event["seq"] = command["seq"].clone();
                            let _ = socket.send(Message::Text(event.to_string().into())).await;
                        }
                    }
                    Err(error) => {
                        let wrapped = voice_asr::VoiceError {
                            code: "asr_task_failed",
                            message: error.to_string(),
                            details: Map::new(),
                        };
                        let _ = send_voice_ws_error(&mut socket, &wrapped).await;
                        break;
                    }
                }
            }
            if recording.as_ref().is_some_and(|value| !value.is_empty()) {
                let Some(active_recording) = recording.take() else {
                    continue;
                };
                let recording_file = match active_recording.finish().await {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = send_voice_ws_error(&mut socket, &error).await;
                        break;
                    }
                };
                let (recording_file, mut final_asr) = voice_final_asr::transcribe_pcm16_file(
                    state.home.clone(),
                    selected.clone(),
                    language.clone(),
                    recording_file,
                )
                .await;
                final_asr["seq"] = command["seq"].clone();
                let _ = socket
                    .send(Message::Text(final_asr.to_string().into()))
                    .await;
                if persist_secretary_transcript {
                    let status = voice_diarization::spawn(
                        voice_diarization::DiarizationJob {
                            state: state.clone(),
                            group_id: group_id.clone(),
                            session_id: client_session_id.clone(),
                            document_path: document_path.clone(),
                            diarization_model: diarization_model.clone(),
                            transcript_model: selected.clone(),
                            language: language.clone(),
                        },
                        recording_file,
                    );
                    let payload = match status {
                        voice_diarization::SpawnStatus::Started => {
                            json!({"type":"diarization_status","ok":true,"seq":command["seq"],"status":"separating_speakers"})
                        }
                        voice_diarization::SpawnStatus::Skipped(reason) => {
                            json!({"type":"diarization_skipped","ok":true,"seq":command["seq"],"reason":reason})
                        }
                    };
                    let _ = socket.send(Message::Text(payload.to_string().into())).await;
                }
            }
            let _ = socket
                .send(Message::Text(
                    json!({"type":"closed","ok":true,"seq":command["seq"]})
                        .to_string()
                        .into(),
                ))
                .await;
            stopped = true;
            break;
        }
    }
    if !stopped {
        voice_ws_lifecycle::finalize_disconnect(
            voice_ws_lifecycle::DisconnectContext {
                state: state.clone(),
                group_id: group_id.clone(),
                client_session_id,
                document_path,
                language,
                final_model_id: selected,
                diarization_model_id: diarization_model,
                persist_artifacts: persist_secretary_transcript,
            },
            session.take(),
            recording.take(),
        )
        .await;
    }
    if let Err(error) = voice_recording_lease::release(&state.home, &group_id, &owner_id, &lease_id)
    {
        tracing::warn!(%error, %group_id, %owner_id, "voice recording lease release failed");
    }
}

async fn recording_lease(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    Ok(success(voice_recording_lease::update(
        &state.home,
        &group_id,
        &body,
    )?))
}

async fn model_install(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let model_id = required(&body, "model_id")?;
    let model = voice_asr::begin_install(state.home.clone(), model_id).map_err(voice_error)?;
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"model":model,"service_runtime":voice_asr::runtime_status()}),
    ))
}
async fn model_remove(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let model_id = required(&body, "model_id")?;
    let model = voice_asr::remove_model(&state.home, &model_id).map_err(voice_error)?;
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"model":model,"service_runtime":voice_asr::runtime_status()}),
    ))
}

async fn latest_session(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<DocumentQuery>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    Ok(success(
        json!({"group_id":group_id,"session":voice_session::latest_document_session(array(&value,"sessions"), &query.document_path)}),
    ))
}
async fn session(
    State(state): State<AppState>,
    Path((group_id, session_id)): Path<(String, String)>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    let session = voice_session::document_session_by_id(array(&value, "sessions"), &session_id)
        .ok_or_else(|| ApiError::not_found("voice session not found"))?;
    Ok(success(json!({"group_id":group_id,"session":session})))
}
async fn clear_latest_transcript(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let document_path = body["document_path"].as_str().unwrap_or("").to_owned();
    let cleared = update(&state, &group_id, |value| {
        let sessions = array_mut(root(value), "sessions");
        Ok(voice_session::clear_latest_document_session(
            sessions,
            &document_path,
        ))
    })?;
    Ok(success(json!({"group_id":group_id,"cleared":cleared})))
}
async fn transcript_segment(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("session_id")
        .or_insert_with(|| json!(format!("vs_{}", short_id())));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_transcript_append", args).await
}

async fn documents(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<DocumentQuery>,
) -> ApiResult {
    call(
        &state,
        "assistant_voice_document_list",
        object(json!({
            "group_id":group_id,
            "document_path":query.document_path,
            "include_archived":query.include_archived,
            "by":"web",
        })),
    )
    .await
}
async fn document_save(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_document_save", args).await
}
async fn document_select(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_document_select", args).await
}
async fn document_instruction(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_document_instruction", args).await
}
async fn document_archive(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_document_archive", args).await
}
async fn input(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let kind = required(&body, "kind")?;
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    match kind.as_str() {
        "voice_instruction" => {
            if args
                .get("instruction")
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
            {
                let text = args.get("text").cloned().unwrap_or(Value::Null);
                args.insert("instruction".into(), text);
            }
            call(&state, "assistant_voice_document_instruction", args).await
        }
        "prompt_refine" => call(&state, "assistant_voice_input_append", args).await,
        _ => Err(ApiError::bad_code(
            "invalid_input_kind",
            format!("unsupported Voice Secretary input kind: {kind}"),
            json!({"kind":kind}),
        )),
    }
}
async fn prompt_ack(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_prompt_draft_ack", args).await
}
async fn clear_asks(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_ask_requests_clear", args).await
}
fn payload(
    state: &AppState,
    group_id: &str,
    value: &Value,
    runtime_assistant: Option<&Value>,
) -> Value {
    let mut assistant = runtime_assistant
        .cloned()
        .unwrap_or_else(|| assistant(value));
    let documents = array(value, "documents").to_vec();
    let asks = array(value, "ask_requests").to_vec();
    let models=voice_asr::list_models(&state.home).unwrap_or_else(|error|vec![json!({"model_id":"","status":"failed","available":false,"error":{"code":error.code,"message":error.message,"details":error.details}})]);
    let models_by_id = models
        .iter()
        .filter_map(|item| {
            item["model_id"]
                .as_str()
                .map(|id| (id.to_owned(), item.clone()))
        })
        .collect::<Map<_, _>>();
    let runtime = voice_asr::runtime_status();
    assistant["health"]["service"] = json!({"status":"ready","alive":true,"asr_command_configured":true,"asr_mock_configured":std::env::var_os("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT").is_some(),"implementation":"rust","runtime":runtime});
    json!({"group_id":group_id,"assistants":[assistant],"assistants_by_id":{"voice_secretary":assistant},"assistant":assistant,"documents":documents,"documents_by_path":documents.iter().filter_map(|item|item["document_path"].as_str().map(|path|(path.to_owned(),item.clone()))).collect::<Map<_,_>>(),"active_document_id":value["active_document_id"],"capture_target_document_id":value["active_document_id"],"active_document_path":value["active_document_path"],"capture_target_document_path":value["active_document_path"],"new_input_available":value["input_latest_seq"].as_u64().unwrap_or(0)>value["input_read_cursor"].as_u64().unwrap_or(0),"prompt_draft":value["prompt_draft"],"ask_requests":asks,"service_models":models,"service_models_by_id":models_by_id,"service_runtime":runtime,"service_runtimes":[runtime],"service_runtimes_by_id":{"sherpa_onnx_streaming":runtime},"recording_lease":voice_recording_lease::current(&state.home)})
}

async fn runtime_assistant(state: &AppState, group_id: &str) -> Option<Value> {
    let response = state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: "assistant_index".into(),
            args: object(json!({"group_id":group_id})),
        })
        .await
        .ok()?;
    response.ok.then(|| response.result["assistant"].clone())
}
pub(super) fn assistant(value: &Value) -> Value {
    value
        .get("assistant")
        .cloned()
        .or_else(|| value.get("voice_secretary").cloned())
        .unwrap_or_else(default_assistant)
}
fn default_assistant() -> Value {
    json!({"assistant_id":"voice_secretary","kind":"voice_secretary","enabled":false,"principal":"assistant:voice_secretary","lifecycle":"disabled","health":{"service":voice_asr::runtime_status()},"policy":{"action_allowlist":[],"requires_user_confirmation":[]},"config":{"capture_mode":"document","recognition_backend":"browser_asr","recognition_language":"auto","retention_ttl_seconds":604800,"auto_document_enabled":true,"document_default_dir":"docs/voice-secretary","auto_document_quiet_ms":1200,"auto_document_min_chars":80,"auto_document_max_window_seconds":30,"service_model_id":voice_asr::DEFAULT_OFFLINE_MODEL_ID,"tts_enabled":false},"ui":{"title":"Voice Secretary"}})
}
fn assistant_mut(root: &mut Map<String, Value>) -> &mut Value {
    let legacy = root.get("voice_secretary").cloned();
    root.entry("assistant")
        .or_insert_with(|| legacy.unwrap_or_else(default_assistant))
}
fn root(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("assistant state initialized");
    assistant_mut(root);
    for key in ["documents", "sessions", "ask_requests", "service_models"] {
        root.entry(key).or_insert_with(|| json!([]));
    }
    root
}
fn voice_error(error: voice_asr::VoiceError) -> ApiError {
    ApiError::bad_code(error.code, error.message, Value::Object(error.details))
}
async fn send_voice_ws_error(
    socket: &mut WebSocket,
    error: &voice_asr::VoiceError,
) -> Result<(), axum::Error> {
    socket.send(Message::Text(json!({"type":"error","ok":false,"error":{"code":error.code,"message":error.message,"details":error.details}}).to_string().into())).await
}
pub(super) fn load(state: &AppState, group_id: &str) -> Result<Value, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    integration_state::group_get(&store, group_id, STORE_KEY)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))
}
fn update<T>(
    state: &AppState,
    group_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    integration_state::group_update(&store, group_id, STORE_KEY, change).map_err(state_error)
}
fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn array_mut<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    root.entry(key)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("array initialized")
}
fn validate_assistant(value: &str) -> Result<(), ApiError> {
    (value == "voice_secretary")
        .then_some(())
        .ok_or_else(|| ApiError::not_found("assistant not found"))
}
fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}
fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_owned()
}
fn state_error(error: io::Error) -> ApiError {
    if error.kind() == io::ErrorKind::NotFound {
        ApiError::not_found(error.to_string())
    } else {
        ApiError::bad(error.to_string())
    }
}
fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
