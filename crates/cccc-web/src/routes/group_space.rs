use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::utc_now;
use cccc_core::integration_state;
use cccc_core::{GroupStore, ledger};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::io;
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

const STORE_KEY: &str = "group_space";

#[derive(Debug, Deserialize)]
struct SpaceQuery {
    #[serde(default = "default_provider")]
    provider: String,
    #[serde(default)]
    lane: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    state: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/space/status", get(status))
        .route("/api/v1/groups/{group_id}/space/spaces", get(spaces))
        .route("/api/v1/groups/{group_id}/space/bind", post(bind))
        .route("/api/v1/groups/{group_id}/space/ingest", post(ingest))
        .route("/api/v1/groups/{group_id}/space/query", post(query))
        .route(
            "/api/v1/groups/{group_id}/space/sources",
            get(list_sources).post(source_action),
        )
        .route(
            "/api/v1/groups/{group_id}/space/artifacts",
            get(list_artifacts).post(artifact_action),
        )
        .route(
            "/api/v1/groups/{group_id}/space/jobs",
            get(list_jobs).post(job_action),
        )
        .route("/api/v1/groups/{group_id}/space/sync", post(sync))
}

async fn status(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    Ok(success(status_payload(&group_id, &query.provider, &value)))
}

async fn spaces(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    let mut remote = Vec::new();
    for binding in value["bindings"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, value)| value)
    {
        let id = binding["remote_space_id"].as_str().unwrap_or("");
        if !id.is_empty()
            && !remote
                .iter()
                .any(|item: &Value| item["remote_space_id"] == id)
        {
            remote.push(json!({"remote_space_id":id,"title":id,"is_owner":true}));
        }
    }
    Ok(success(
        json!({"group_id":group_id,"provider":query.provider,"provider_state":provider_state(&query.provider),"bindings":value["bindings"],"spaces":remote}),
    ))
}

async fn bind(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let provider = provider(&body);
    let lane = lane(&body)?;
    let action = body["action"].as_str().unwrap_or("bind");
    let remote = body["remote_space_id"].as_str().unwrap_or("").trim();
    if action == "bind" && remote.is_empty() {
        return Err(ApiError::bad("remote_space_id is required"));
    }
    update(&state, &group_id, |value| {
        let bindings = map_field(root(value), "bindings");
        if action == "unbind" {
            bindings.remove(&lane);
        } else {
            bindings.insert(lane.clone(), json!({"group_id":group_id,"provider":provider,"lane":lane,"remote_space_id":remote,"bound_by":"user","bound_at":utc_now(),"status":"bound"}));
        }
        Ok(())
    })?;
    Ok(success(status_payload(
        &group_id,
        &provider,
        &load(&state, &group_id)?,
    )))
}

async fn ingest(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let provider = provider(&body);
    let lane = lane(&body)?;
    let kind = body["kind"].as_str().unwrap_or("context_sync").to_owned();
    let payload = body.get("payload").cloned().unwrap_or_else(|| json!({}));
    let idempotency = body["idempotency_key"].as_str().unwrap_or("").to_owned();
    let (job, deduped) = update(&state, &group_id, |value| {
        let root = root(value);
        if !idempotency.is_empty()
            && let Some(job) = array_field(root, "jobs")
                .iter()
                .find(|job| job["idempotency_key"] == idempotency)
        {
            return Ok((job.clone(), true));
        }
        let job = json!({"job_id":format!("gsj_{}",short_id()),"group_id":group_id,"provider":provider,"lane":lane,"remote_space_id":binding_id(root,&lane),"kind":kind,"payload":payload,"idempotency_key":idempotency,"state":"succeeded","attempt":1,"max_attempts":3,"created_at":utc_now(),"updated_at":utc_now(),"execution_mode":"local_fallback"});
        array_field(root, "jobs").push(job.clone());
        let title = payload["title"]
            .as_str()
            .or_else(|| payload["path"].as_str())
            .unwrap_or(&kind);
        array_field(root,"sources").push(json!({"source_id":format!("gss_{}",short_id()),"provider":provider,"lane":lane,"title":title,"kind":kind,"status":"ready","payload":payload,"created_at":utc_now()}));
        Ok((job, false))
    })?;
    Ok(success(
        json!({"group_id":group_id,"job_id":job["job_id"],"accepted":true,"completed":true,"deduped":deduped,"job":job,"queue_summary":queue_summary(&load(&state,&group_id)?),"provider_mode":"local_fallback","degraded":true}),
    ))
}

async fn query(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let provider = provider(&body);
    let lane = lane(&body)?;
    let query = body["query"].as_str().unwrap_or("").trim();
    if query.is_empty() {
        return Err(ApiError::bad("query is required"));
    }
    let value = load(&state, &group_id)?;
    let terms = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let mut references = array(&value, "sources").iter().filter(|source| source["lane"] == lane).filter_map(|source| {
        let text=source.to_string(); let lower=text.to_ascii_lowercase(); terms.iter().any(|term|lower.contains(term)).then(||json!({"source_id":source["source_id"],"title":source["title"],"excerpt":truncate(&text,400)}))
    }).take(8).collect::<Vec<_>>();
    if references.is_empty() {
        let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
        let events =
            ledger::tail(&store.ledger_path(&group_id).map_err(io_error)?, 50).map_err(io_error)?;
        references.extend(events.into_iter().rev().filter_map(|event|{let text=serde_json::to_string(&event).ok()?;let lower=text.to_ascii_lowercase();terms.iter().any(|term|lower.contains(term)).then(||json!({"event_id":event.id,"kind":event.kind,"excerpt":truncate(&text,400)}))}).take(8));
    }
    let answer = if references.is_empty() {
        "No matching material was found in the selected Group Space lane.".to_owned()
    } else {
        references
            .iter()
            .enumerate()
            .map(|(index, item)| {
                format!("{}. {}", index + 1, item["excerpt"].as_str().unwrap_or(""))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(success(
        json!({"group_id":group_id,"provider":provider,"provider_mode":"local_fallback","degraded":true,"answer":answer,"references":references,"error":null}),
    ))
}

async fn list_sources(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    let sources = filtered(&value, "sources", &query.provider, &query.lane, "");
    Ok(success(
        json!({"group_id":group_id,"provider":query.provider,"provider_mode":"local_fallback","binding":value["bindings"][&query.lane],"action":"list","sources":sources}),
    ))
}
async fn source_action(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let source_id = required(&body, "source_id")?;
    let action = body["action"].as_str().unwrap_or("refresh").to_owned();
    let changed = update(&state, &group_id, |value| {
        let sources = array_field(root(value), "sources");
        if action == "delete" {
            let before = sources.len();
            sources.retain(|item| item["source_id"] != source_id);
            return Ok(before != sources.len());
        }
        let item = sources
            .iter_mut()
            .find(|item| item["source_id"] == source_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "source not found"))?;
        if action == "rename" {
            item["title"] = body.get("new_title").cloned().unwrap_or(json!(""));
        } else {
            item["updated_at"] = json!(utc_now());
        }
        Ok(true)
    })?;
    Ok(success(
        json!({"group_id":group_id,"provider":provider(&body),"provider_mode":"local_fallback","action":action,"source_id":source_id,"changed":changed}),
    ))
}
async fn list_artifacts(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    let artifacts = filtered(
        &value,
        "artifacts",
        &query.provider,
        &query.lane,
        &query.kind,
    );
    Ok(success(
        json!({"group_id":group_id,"provider":query.provider,"provider_mode":"local_fallback","binding":value["bindings"][&query.lane],"action":"list","kind":query.kind,"artifacts":artifacts}),
    ))
}
async fn artifact_action(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let action = body["action"].as_str().unwrap_or("generate").to_owned();
    if action != "download" {
        return Err(ApiError::unavailable(
            "provider_unavailable",
            "NotebookLM artifact generation is unavailable; no remote operation was performed",
        ));
    }
    let kind = required(&body, "kind")?;
    let lane = lane(&body)?;
    let provider = provider(&body);
    let artifact = update(&state, &group_id, |value| {
        let root = root(value);
        if action == "download" {
            return array_field(root, "artifacts")
                .iter()
                .find(|item| {
                    body["artifact_id"]
                        .as_str()
                        .is_some_and(|id| item["artifact_id"] == id)
                })
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "artifact not found"));
        }
        let item = json!({"artifact_id":format!("gsa_{}",short_id()),"provider":provider,"lane":lane,"title":format!("{} artifact",kind),"kind":kind,"status":"completed","created_at":utc_now(),"url":""});
        array_field(root, "artifacts").push(item.clone());
        Ok(item)
    })?;
    Ok(success(
        json!({"group_id":group_id,"provider":provider,"provider_mode":"local_fallback","action":action,"kind":kind,"accepted":true,"completed":true,"artifact_id":artifact["artifact_id"],"generate_result":artifact,"download_result":artifact}),
    ))
}
async fn list_jobs(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SpaceQuery>,
) -> ApiResult {
    let value = load(&state, &group_id)?;
    let jobs = filtered(&value, "jobs", &query.provider, &query.lane, "")
        .into_iter()
        .filter(|item| query.state.is_empty() || item["state"] == query.state)
        .take(query.limit.clamp(1, 500))
        .collect::<Vec<_>>();
    Ok(success(
        json!({"group_id":group_id,"provider":query.provider,"jobs":jobs,"queue_summary":queue_summary(&value)}),
    ))
}
async fn job_action(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let job_id = required(&body, "job_id")?;
    let action = body["action"].as_str().unwrap_or("retry");
    let job = update(&state, &group_id, |value| {
        let item = array_field(root(value), "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == job_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        item["state"] = json!(if action == "cancel" {
            "canceled"
        } else {
            "succeeded"
        });
        if action == "retry" {
            item["attempt"] = json!(item["attempt"].as_u64().unwrap_or(0) + 1);
        }
        item["updated_at"] = json!(utc_now());
        Ok(item.clone())
    })?;
    Ok(success(
        json!({"group_id":group_id,"provider":provider(&body),"job":job,"queue_summary":queue_summary(&load(&state,&group_id)?)}),
    ))
}
async fn sync(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let _ = lane(&body)?;
    let _ = load(&state, &group_id)?;
    Err(ApiError::unavailable(
        "provider_unavailable",
        "NotebookLM sync is unavailable; no remote operation was performed",
    ))
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
fn status_payload(group_id: &str, provider: &str, value: &Value) -> Value {
    json!({"group_id":group_id,"provider":provider_state(provider),"bindings":value.get("bindings").cloned().unwrap_or(json!({})),"queue_summary":{"work":queue_summary(value),"memory":queue_summary(value)},"sync":value.get("sync").cloned().unwrap_or(json!({"available":false,"converged":false,"reason":"provider_unavailable"}))})
}
fn provider_state(provider: &str) -> Value {
    json!({"provider":provider,"enabled":true,"real_enabled":false,"mode":"local_fallback","real_adapter_enabled":false,"stub_adapter_enabled":true,"auth_configured":false,"write_ready":false,"readiness_reason":"remote provider unavailable; local query and ingest only"})
}
fn filtered(value: &Value, section: &str, provider: &str, lane: &str, kind: &str) -> Vec<Value> {
    array(value, section)
        .iter()
        .filter(|item| {
            (provider.is_empty() || item["provider"] == provider)
                && (lane.is_empty() || item["lane"] == lane)
                && (kind.is_empty() || item["kind"] == kind)
        })
        .cloned()
        .collect()
}
fn queue_summary(value: &Value) -> Value {
    let jobs = array(value, "jobs");
    json!({"pending":jobs.iter().filter(|item|item["state"]=="pending").count(),"running":jobs.iter().filter(|item|item["state"]=="running").count(),"failed":jobs.iter().filter(|item|item["state"]=="failed").count()})
}
fn root(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("space state initialized");
    for key in ["bindings", "sources", "artifacts", "jobs"] {
        root.entry(key).or_insert_with(|| {
            if key == "bindings" {
                json!({})
            } else {
                json!([])
            }
        });
    }
    root
}
fn map_field<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("map initialized")
}
fn array_field<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    root.get_mut(key)
        .and_then(Value::as_array_mut)
        .expect("array initialized")
}
fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn binding_id(root: &Map<String, Value>, lane: &str) -> String {
    root.get("bindings")
        .and_then(|value| value.get(lane))
        .and_then(|value| value["remote_space_id"].as_str())
        .unwrap_or("")
        .to_owned()
}
fn provider(body: &Value) -> String {
    body["provider"]
        .as_str()
        .unwrap_or("notebooklm")
        .trim()
        .to_owned()
}
fn lane(body: &Value) -> Result<String, ApiError> {
    let lane = required(body, "lane")?;
    matches!(lane.as_str(), "work" | "memory")
        .then_some(lane)
        .ok_or_else(|| ApiError::bad("lane must be work or memory"))
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
fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
fn default_provider() -> String {
    "notebooklm".into()
}
fn default_limit() -> usize {
    50
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
