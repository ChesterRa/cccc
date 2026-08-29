use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_core::{assistant_state, voice_recording_lease};
use serde::Deserialize;
use serde_json::{Map, Value, json};
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
mod voice_segment_analysis;
mod voice_segmented_recording;
mod voice_speaker_transcript;
mod voice_ws;
mod voice_ws_capture;
mod voice_ws_lifecycle;

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
    let runtime_state = runtime_state(&state, &group_id).await?;
    Ok(success(payload(&state, &group_id, &runtime_state)))
}
async fn show(
    State(state): State<AppState>,
    Path((group_id, assistant_id)): Path<(String, String)>,
) -> ApiResult {
    if assistant_id != "voice_secretary" {
        return Err(ApiError::not_found("assistant not found"));
    }
    let runtime_state = runtime_state(&state, &group_id).await?;
    Ok(success(payload(&state, &group_id, &runtime_state)))
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
    let recording_lease = voice_recording_lease::validate(
        &state.home,
        &group_id,
        query.owner_id.trim(),
        query.lease_id.trim(),
    )
    .map_err(lease_error)?;
    Ok(ws.on_upgrade(move |socket| {
        voice_ws::serve(
            state,
            group_id,
            query.owner_id,
            query.lease_id,
            recording_lease,
            socket,
        )
    }))
}

async fn recording_lease(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_recording_lease", args).await
}

async fn model_install(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let model_id = required(&body, "model_id")?;
    let model = voice_asr::begin_install(state.home.clone(), model_id).map_err(voice_error)?;
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"model":model}),
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
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"model":model}),
    ))
}

async fn latest_session(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<DocumentQuery>,
) -> ApiResult {
    call(
        &state,
        "assistant_state",
        object(json!({
            "group_id":group_id,
            "assistant_id":"voice_secretary",
            "view":"voice_session",
            "document_path":query.document_path,
            "suppress_retry_notify":true
        })),
    )
    .await
}
async fn session(
    State(state): State<AppState>,
    Path((group_id, session_id)): Path<(String, String)>,
) -> ApiResult {
    call(
        &state,
        "assistant_state",
        object(json!({
            "group_id":group_id,
            "assistant_id":"voice_secretary",
            "view":"voice_session",
            "session_id":session_id,
            "suppress_retry_notify":true
        })),
    )
    .await
}
async fn clear_latest_transcript(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = object(body);
    args.insert("group_id".into(), json!(group_id));
    args.entry("by").or_insert_with(|| json!("user"));
    call(&state, "assistant_voice_session_transcript_clear", args).await
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
fn payload(state: &AppState, group_id: &str, value: &Value) -> Value {
    let mut assistant = assistant(value);
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
    let requested_model_id = assistant["config"]["service_model_id"]
        .as_str()
        .unwrap_or("");
    let mut streaming_backend =
        voice_asr::streaming_backend_status(&state.home, requested_model_id);
    let asr_mock_configured = std::env::var("CCCC_VOICE_SECRETARY_ASR_MOCK_TEXT")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    if asr_mock_configured {
        streaming_backend = json!({"ready":true,"status":"ready","model_id":"mock"});
    }
    let service_ready = streaming_backend["ready"].as_bool().unwrap_or(false);
    let selected_model = streaming_backend["model_id"]
        .as_str()
        .and_then(|model_id| models_by_id.get(model_id))
        .cloned()
        .unwrap_or_else(|| json!({}));
    assistant["health"]["service"] = json!({
        "status":if service_ready {"ready"} else {"unavailable"},
        "alive":true,
        "ready":service_ready,
        "mock":asr_mock_configured,
        "selected_model_id":streaming_backend["model_id"],
        "streaming_backend":streaming_backend,
        "model":selected_model,
        "runtime":runtime
    });
    json!({"group_id":group_id,"assistants":[assistant],"assistants_by_id":{"voice_secretary":assistant},"assistant":assistant,"documents":documents,"documents_by_path":documents.iter().filter_map(|item|item["document_path"].as_str().map(|path|(path.to_owned(),item.clone()))).collect::<Map<_,_>>(),"active_document_id":value["active_document_id"],"capture_target_document_id":value["active_document_id"],"active_document_path":value["active_document_path"],"capture_target_document_path":value["active_document_path"],"new_input_available":value["new_input_available"].as_bool().unwrap_or_else(||value["input_latest_seq"].as_u64().unwrap_or(0)>value["input_read_cursor"].as_u64().unwrap_or(0)),"prompt_draft":value["prompt_draft"],"ask_requests":asks,"service_models":models,"service_models_by_id":models_by_id,"service_runtime":runtime,"recording_lease":voice_recording_lease::current(&state.home).unwrap_or_else(|_|json!({}))})
}

async fn runtime_state(state: &AppState, group_id: &str) -> Result<Value, ApiError> {
    let Json(response) = call(
        state,
        "assistant_state",
        object(json!({
            "group_id":group_id,
            "assistant_id":"voice_secretary",
            "view":"voice_workspace",
            "suppress_retry_notify":true,
        })),
    )
    .await?;
    Ok(response["result"].clone())
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
fn voice_error(error: voice_asr::VoiceError) -> ApiError {
    ApiError::bad_code(error.code, error.message, Value::Object(error.details))
}
fn lease_error(error: voice_recording_lease::LeaseError) -> ApiError {
    let details = Value::Object(error.details);
    if matches!(
        error.code,
        "assistant_voice_recording_busy" | "assistant_voice_recording_lease_lost"
    ) {
        return ApiError::conflict(error.code, error.message, details);
    }
    ApiError::bad_code(error.code, error.message, details)
}
async fn send_voice_ws_error(
    socket: &mut WebSocket,
    error: &voice_asr::VoiceError,
) -> Result<(), axum::Error> {
    socket.send(Message::Text(json!({"type":"error","ok":false,"error":{"code":error.code,"message":error.message,"details":error.details}}).to_string().into())).await
}
pub(super) fn load(state: &AppState, group_id: &str) -> Result<Value, ApiError> {
    assistant_state::load(&state.home, group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))
}
fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
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
