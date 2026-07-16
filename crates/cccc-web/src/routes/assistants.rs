use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use cccc_contracts::utc_now;
use cccc_core::GroupStore;
use cccc_core::integration_state;
use cccc_core::settings;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io;
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

mod voice_recording_lease;

const STORE_KEY: &str = "assistants";

#[derive(Debug, Default, Deserialize)]
struct DocumentQuery {
    #[serde(default)]
    document_path: String,
    #[serde(default)]
    include_archived: bool,
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
            post(transcribe),
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
            post(runtime_install),
        )
        .route(
            "/api/v1/groups/{group_id}/assistants/voice_secretary/runtime/remove",
            post(runtime_remove),
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
    Ok(success(payload(
        &state,
        &group_id,
        &load(&state, &group_id)?,
    )))
}
async fn show(
    State(state): State<AppState>,
    Path((group_id, assistant_id)): Path<(String, String)>,
) -> ApiResult {
    if assistant_id != "voice_secretary" {
        return Err(ApiError::not_found("assistant not found"));
    }
    Ok(success(payload(
        &state,
        &group_id,
        &load(&state, &group_id)?,
    )))
}
async fn update_settings(
    State(state): State<AppState>,
    Path((group_id, assistant_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_assistant(&assistant_id)?;
    let assistant = update(&state, &group_id, |value| {
        let root = root(value);
        let assistant = assistant_mut(root);
        if let Some(enabled) = body["enabled"].as_bool() {
            assistant["enabled"] = json!(enabled);
            assistant["lifecycle"] = json!(if enabled { "running" } else { "disabled" });
        }
        if let Some(patch) = body["config"].as_object() {
            let config = assistant
                .get_mut("config")
                .and_then(Value::as_object_mut)
                .expect("config initialized");
            settings::merge(config, patch);
        }
        assistant["updated_at"] = json!(utc_now());
        Ok(assistant.clone())
    })?;
    Ok(success(json!({"group_id":group_id,"assistant":assistant})))
}
async fn update_status(
    State(state): State<AppState>,
    Path((group_id, assistant_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_assistant(&assistant_id)?;
    let assistant = update(&state, &group_id, |value| {
        let assistant = assistant_mut(root(value));
        assistant["lifecycle"] = body.get("lifecycle").cloned().unwrap_or(json!("idle"));
        assistant["health"] = body.get("health").cloned().unwrap_or(json!({}));
        assistant["updated_at"] = json!(utc_now());
        Ok(assistant.clone())
    })?;
    Ok(success(json!({"group_id":group_id,"assistant":assistant})))
}
async fn transcribe(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let raw = body["audio_base64"].as_str().unwrap_or("");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| ApiError::bad("audio_base64 is invalid"))?;
    let assistant = assistant(&load(&state, &group_id)?);
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant,"transcript":"","mime_type":body["mime_type"],"language":body["language"],"bytes":bytes.len(),"backend":"unavailable","service":{"available":false,"reason":"sherpa-onnx runtime is not installed"},"asr":{"available":false}}),
    ))
}
async fn transcription_ws(Path(_group_id): Path<String>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(serve_transcription_ws)
}

async fn serve_transcription_ws(mut socket: WebSocket) {
    while let Some(Ok(message)) = socket.recv().await {
        if matches!(message, Message::Close(_)) {
            break;
        }
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(command) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if command["type"] == "start" {
            let _ = socket
                .send(Message::Text(
                    json!({"type":"error","ok":false,"error":{"code":"asr_unavailable","message":"sherpa-onnx runtime is not installed"}})
                        .to_string()
                        .into(),
                ))
                .await;
            break;
        }
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
    model_change(&state, &group_id, &body, true)
}
async fn model_remove(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    model_change(&state, &group_id, &body, false)
}
fn model_change(state: &AppState, group_id: &str, body: &Value, install: bool) -> ApiResult {
    let model_id = body["model_id"]
        .as_str()
        .unwrap_or("sherpa-onnx")
        .to_owned();
    let model = update(state, group_id, |value| {
        let models = array_mut(root(value), "service_models");
        if install {
            models.retain(|item| item["model_id"] != model_id);
            models.push(json!({"model_id":model_id,"status":"unavailable","installed":false,"reason":"Install sherpa-onnx model files externally and configure their path."}));
        } else {
            models.retain(|item| item["model_id"] != model_id);
        }
        Ok(
            json!({"model_id":model_id,"status":if install{"unavailable"}else{"removed"},"installed":false}),
        )
    })?;
    Ok(success(json!({
        "group_id":group_id,"assistant":assistant(&load(state,group_id)?),
        "model":model,"service_runtime":runtime_payload(false)
    })))
}
async fn runtime_install(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(_body): Json<Value>,
) -> ApiResult {
    update(&state, &group_id, |value| {
        root(value).insert("service_runtime".into(), runtime_payload(false));
        Ok(())
    })?;
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"service_runtime":runtime_payload(false)}),
    ))
}
async fn runtime_remove(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(_body): Json<Value>,
) -> ApiResult {
    update(&state, &group_id, |value| {
        root(value).remove("service_runtime");
        Ok(())
    })?;
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"service_runtime":runtime_payload(false)}),
    ))
}

async fn latest_session(State(state): State<AppState>, Path(group_id): Path<String>) -> ApiResult {
    let value = load(&state, &group_id)?;
    Ok(success(
        json!({"group_id":group_id,"session":array(&value,"sessions").last().cloned()}),
    ))
}
async fn session(
    State(state): State<AppState>,
    Path((group_id, session_id)): Path<(String, String)>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    let session = array(&value, "sessions")
        .iter()
        .find(|item| item["session_id"] == session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("voice session not found"))?;
    Ok(success(json!({"group_id":group_id,"session":session})))
}
async fn clear_latest_transcript(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(_body): Json<Value>,
) -> ApiResult {
    let cleared = update(&state, &group_id, |value| {
        let sessions = array_mut(root(value), "sessions");
        if let Some(session) = sessions.last_mut() {
            session["segments"] = json!([]);
            session["transcript"] = json!("");
            return Ok(true);
        }
        Ok(false)
    })?;
    Ok(success(json!({"group_id":group_id,"cleared":cleared})))
}
async fn transcript_segment(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let session_id = body["session_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("vs_{}", short_id()));
    let text = body["text"].as_str().unwrap_or("").to_owned();
    let document_path = body["document_path"].as_str().unwrap_or("").to_owned();
    let result = update(&state, &group_id, |value| {
        let root = root(value);
        let sessions = array_mut(root, "sessions");
        let index=sessions.iter().position(|item|item["session_id"]==session_id).unwrap_or_else(||{sessions.push(json!({"session_id":session_id,"created_at":utc_now(),"updated_at":utc_now(),"segments":[],"transcript":""}));sessions.len()-1});
        let segment = json!({"segment_id":body["segment_id"],"text":text,"language":body["language"],"is_final":body["is_final"],"start_ms":body["start_ms"],"end_ms":body["end_ms"],"speaker_label":body["speaker_label"],"created_at":utc_now()});
        let session = &mut sessions[index];
        let segments = session
            .get_mut("segments")
            .and_then(Value::as_array_mut)
            .expect("segments initialized");
        segments.push(segment.clone());
        session["transcript"] = json!(
            segments
                .iter()
                .filter_map(|item| item["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        session["updated_at"] = json!(utc_now());
        let document = if document_path.is_empty() {
            Value::Null
        } else {
            upsert_document(root, &document_path, "", Some(&text), false)
        };
        Ok((segment, document))
    })?;
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"session_id":session_id,"segment":result.0,"document":result.1,"document_updated":result.1.is_object(),"input_event_created":false,"input_notify_emitted":false,"actor_woken":false,"actor_notify_delivered":false}),
    ))
}

async fn documents(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<DocumentQuery>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    let docs = array(&value, "documents")
        .iter()
        .filter(|item| {
            (query.include_archived || item["status"] != "archived")
                && (query.document_path.is_empty() || item["document_path"] == query.document_path)
        })
        .cloned()
        .collect::<Vec<_>>();
    Ok(success(
        json!({"group_id":group_id,"documents":docs,"active_document_id":value["active_document_id"],"active_document_path":value["active_document_path"]}),
    ))
}
async fn document_save(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let path = body["document_path"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("voice/{}.md", short_id()));
    validate_document_path(&path)?;
    let document = update(&state, &group_id, |value| {
        let root = root(value);
        let document = upsert_document(
            root,
            &path,
            body["title"].as_str().unwrap_or(""),
            body.get("content").and_then(Value::as_str),
            body["create_new"].as_bool().unwrap_or(false),
        );
        root.insert("active_document_id".into(), document["document_id"].clone());
        root.insert("active_document_path".into(), json!(path));
        Ok(document)
    })?;
    Ok(success(document_result(
        &group_id,
        assistant(&load(&state, &group_id)?),
        document,
    )))
}
async fn document_select(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let path = required(&body, "document_path")?;
    let document = update(&state, &group_id, |value| {
        let root = root(value);
        let document = array_mut(root, "documents")
            .iter()
            .find(|item| item["document_path"] == path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
        root.insert("active_document_id".into(), document["document_id"].clone());
        root.insert("active_document_path".into(), json!(path));
        Ok(document)
    })?;
    Ok(success(document_result(
        &group_id,
        assistant(&load(&state, &group_id)?),
        document,
    )))
}
async fn document_instruction(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let path = required(&body, "document_path")?;
    let instruction = required(&body, "instruction")?;
    let request = input_record(&state, &group_id, "voice_instruction", &instruction, &path)?;
    Ok(success(document_result_extra(
        &group_id,
        assistant(&load(&state, &group_id)?),
        request.0,
        request.1,
    )))
}
async fn document_archive(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let path = required(&body, "document_path")?;
    let document = update(&state, &group_id, |value| {
        let item = array_mut(root(value), "documents")
            .iter_mut()
            .find(|item| item["document_path"] == path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
        item["status"] = json!("archived");
        item["updated_at"] = json!(utc_now());
        Ok(item.clone())
    })?;
    Ok(success(document_result(
        &group_id,
        assistant(&load(&state, &group_id)?),
        document,
    )))
}
async fn input(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let kind = required(&body, "kind")?;
    let text = body["text"]
        .as_str()
        .or_else(|| body["instruction"].as_str())
        .or_else(|| body["voice_transcript"].as_str())
        .unwrap_or("");
    let path = body["document_path"].as_str().unwrap_or("");
    let result = input_record(&state, &group_id, &kind, text, path)?;
    Ok(success(document_result_extra(
        &group_id,
        assistant(&load(&state, &group_id)?),
        result.0,
        result.1,
    )))
}
async fn prompt_ack(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let request_id = required(&body, "request_id")?;
    let status = required(&body, "status")?;
    let draft = update(&state, &group_id, |value| {
        let root = root(value);
        let draft = root
            .get_mut("prompt_draft")
            .filter(|item| item["request_id"] == request_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "prompt draft not found"))?;
        draft["status"] = json!(status);
        Ok(draft.clone())
    })?;
    Ok(success(
        json!({"group_id":group_id,"assistant":assistant(&load(&state,&group_id)?),"prompt_draft":draft}),
    ))
}
async fn clear_asks(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let keep = body["keep_active"].as_bool().unwrap_or(false);
    update(&state, &group_id, |value| {
        let asks = array_mut(root(value), "ask_requests");
        asks.retain(|item| {
            keep && matches!(
                item["status"].as_str(),
                Some("pending" | "working" | "needs_user")
            )
        });
        Ok(())
    })?;
    Ok(success(payload(
        &state,
        &group_id,
        &load(&state, &group_id)?,
    )))
}

fn input_record(
    state: &AppState,
    group_id: &str,
    kind: &str,
    text: &str,
    path: &str,
) -> Result<(Value, Value), ApiError> {
    update(state, group_id, |value| {
        let root = root(value);
        let document = if path.is_empty() {
            Value::Null
        } else {
            array_mut(root, "documents")
                .iter()
                .find(|item| item["document_path"] == path)
                .cloned()
                .unwrap_or(Value::Null)
        };
        let request_id = format!("var_{}", short_id());
        let request = json!({"request_id":request_id,"kind":kind,"request_text":text,"document_path":path,"status":"pending","created_at":utc_now(),"updated_at":utc_now()});
        if kind == "prompt_refine" {
            root.insert("prompt_draft".into(),json!({"request_id":request_id,"status":"pending","operation":"refine","draft_text":text,"draft_preview":text,"created_at":utc_now()}));
        } else {
            array_mut(root, "ask_requests").push(request.clone());
        }
        Ok((document, request))
    })
}
fn payload(state: &AppState, group_id: &str, value: &Value) -> Value {
    let assistant = assistant(value);
    let documents = array(value, "documents").to_vec();
    let asks = array(value, "ask_requests").to_vec();
    json!({"group_id":group_id,"assistants":[assistant],"assistants_by_id":{"voice_secretary":assistant},"assistant":assistant,"documents":documents,"active_document_id":value["active_document_id"],"capture_target_document_id":value["active_document_id"],"active_document_path":value["active_document_path"],"capture_target_document_path":value["active_document_path"],"new_input_available":!asks.is_empty(),"prompt_draft":value["prompt_draft"],"ask_requests":asks,"service_models":array(value,"service_models"),"service_runtime":value["service_runtime"],"recording_lease":voice_recording_lease::current(&state.home)})
}
fn assistant(value: &Value) -> Value {
    value
        .get("assistant")
        .cloned()
        .unwrap_or_else(default_assistant)
}
fn default_assistant() -> Value {
    json!({"assistant_id":"voice_secretary","kind":"voice_secretary","enabled":false,"principal":"assistant:voice_secretary","lifecycle":"disabled","health":{},"policy":{"action_allowlist":[],"requires_user_confirmation":[]},"config":{"capture_mode":"document","recognition_backend":"browser"},"ui":{"title":"Voice Secretary"}})
}
fn assistant_mut(root: &mut Map<String, Value>) -> &mut Value {
    root.entry("assistant").or_insert_with(default_assistant)
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
fn upsert_document(
    root: &mut Map<String, Value>,
    path: &str,
    title: &str,
    content: Option<&str>,
    create_new: bool,
) -> Value {
    let docs = array_mut(root, "documents");
    let index = (!create_new)
        .then(|| docs.iter().position(|item| item["document_path"] == path))
        .flatten();
    let old = index
        .and_then(|index| docs.get(index))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = content.unwrap_or_else(|| old["content"].as_str().unwrap_or(""));
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    let document = json!({"document_id":old["document_id"].as_str().map(str::to_owned).unwrap_or_else(||format!("vdoc_{}",short_id())),"document_path":path,"workspace_path":path,"filename":path.rsplit('/').next().unwrap_or(path),"assistant_id":"voice_secretary","title":if title.is_empty(){old["title"].as_str().unwrap_or("Untitled document")}else{title},"status":old["status"].as_str().unwrap_or("active"),"storage_kind":"rust_home","content":text,"content_sha256":digest,"content_chars":text.chars().count(),"revision_count":old["revision_count"].as_u64().unwrap_or(0)+1,"created_at":old["created_at"].as_str().unwrap_or(""),"updated_at":utc_now(),"created_by":"user"});
    if let Some(index) = index {
        docs[index] = document.clone();
    } else {
        docs.push(document.clone());
    }
    document
}
fn document_result(group_id: &str, assistant: Value, document: Value) -> Value {
    json!({"group_id":group_id,"assistant":assistant,"document":document,"input_event_created":false,"input_notify_emitted":false,"actor_woken":false,"actor_notify_delivered":false})
}
fn document_result_extra(
    group_id: &str,
    assistant: Value,
    document: Value,
    request: Value,
) -> Value {
    let mut result = document_result(group_id, assistant, document);
    result["request_id"] = request["request_id"].clone();
    result["input_event"] = request;
    result
}
fn runtime_payload(installed: bool) -> Value {
    json!({"runtime_id":"sherpa-onnx","installed":installed,"status":if installed{"ready"}else{"unavailable"},"available":installed,"reason":if installed{""}else{"sherpa-onnx runtime is not bundled"}})
}
fn load(state: &AppState, group_id: &str) -> Result<Value, ApiError> {
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
fn validate_document_path(value: &str) -> Result<(), ApiError> {
    let path = std::path::Path::new(value);
    (!path.is_absolute()
        && !value.contains('\0')
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)))
    .then_some(())
    .ok_or_else(|| ApiError::bad("invalid document_path"))
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
