use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::{ActorRuntime, utc_now};
use cccc_core::GroupStore;
use cccc_core::integration_state;
use serde_json::{Value, json};
use std::io;
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

const STORE_KEY: &str = "web_model_connectors";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/web-model/connectors", get(list).post(create))
        .route(
            "/api/v1/web-model/connectors/{connector_id}",
            axum::routing::delete(revoke),
        )
        .route("/api/v1/mcp", post(admin_mcp))
        .route(
            "/mcp/web-model/{connector_id}",
            get(mcp_info).post(mcp_with_header).options(mcp_options),
        )
        .route(
            "/mcp/web-model/{connector_id}/token/{secret}",
            get(mcp_info_token)
                .post(mcp_with_path_token)
                .options(mcp_options_token),
        )
}

async fn list(State(state): State<AppState>) -> ApiResult {
    let mut connectors = load(&state)?;
    connectors.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    Ok(success(json!({
        "connectors": connectors.iter().map(public).collect::<Vec<_>>()
    })))
}

async fn create(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    let group = store
        .load(&group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| ApiError::not_found(format!("actor not found: {actor_id}")))?;
    if actor.runtime != ActorRuntime::WebModel {
        return Err(ApiError::bad(
            "web-model connectors require an actor with runtime=web_model",
        ));
    }

    let connector_id = format!("wmc_{}", &Uuid::new_v4().simple().to_string()[..16]);
    let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let now = utc_now();
    let provider = body
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("chatgpt")
        .trim();
    let connector = json!({
        "connector_id":connector_id,
        "kind":"web_model",
        "group_id":group_id,
        "actor_id":actor_id,
        "provider":if provider.is_empty(){"chatgpt"}else{provider},
        "label":body.get("label").and_then(Value::as_str).unwrap_or(""),
        "secret":secret,
        "created_at":now,
        "updated_at":now,
        "revoked":false
    });
    let replaced = integration_state::global_update(&state.home, STORE_KEY, |value| {
        let items = ensure_array(value);
        let mut replaced = Vec::new();
        items.retain(|item| {
            let same = item["group_id"] == connector["group_id"]
                && item["actor_id"] == connector["actor_id"]
                && !item["revoked"].as_bool().unwrap_or(false);
            if same {
                if let Some(id) = item["connector_id"].as_str() {
                    replaced.push(id.to_owned());
                }
            }
            !same
        });
        items.push(connector.clone());
        Ok(replaced)
    })
    .map_err(io_error)?;
    Ok(success(json!({
        "connector": public(&connector),
        "secret": secret,
        "replaced_connector_ids": replaced
    })))
}

async fn revoke(State(state): State<AppState>, Path(connector_id): Path<String>) -> ApiResult {
    let found = integration_state::global_update(&state.home, STORE_KEY, |value| {
        let mut found = false;
        for item in ensure_array(value) {
            if item["connector_id"] == connector_id {
                item["revoked"] = Value::Bool(true);
                item["updated_at"] = Value::String(utc_now());
                found = true;
            }
        }
        Ok(found)
    })
    .map_err(io_error)?;
    if !found {
        return Err(ApiError::not_found("web-model connector not found"));
    }
    Ok(success(json!({"revoked":true,"connector_id":connector_id})))
}

async fn admin_mcp(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(cccc_mcp::handle_request(&state.home, &body).await)
}

async fn mcp_info(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let secret = bearer(&headers).ok_or_else(|| ApiError::forbidden("connector token required"))?;
    let connector = find_authorized(&state, &connector_id, Some(secret))?;
    Ok(Json(info_payload(&connector)))
}

async fn mcp_info_token(
    State(state): State<AppState>,
    Path((connector_id, secret)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let connector = find_authorized(&state, &connector_id, Some(&secret))?;
    Ok(Json(info_payload(&connector)))
}

async fn mcp_with_header(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let secret = bearer(&headers).ok_or_else(|| ApiError::forbidden("connector token required"))?;
    let connector = find_authorized(&state, &connector_id, Some(secret))?;
    run_connector_mcp(&state, &connector, body).await
}

async fn mcp_with_path_token(
    State(state): State<AppState>,
    Path((connector_id, secret)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let connector = find_authorized(&state, &connector_id, Some(&secret))?;
    run_connector_mcp(&state, &connector, body).await
}

async fn run_connector_mcp(
    state: &AppState,
    connector: &Value,
    mut request: Value,
) -> Result<Json<Value>, ApiError> {
    if request.get("method").and_then(Value::as_str) == Some("tools/call") {
        let arguments = request
            .get_mut("params")
            .and_then(Value::as_object_mut)
            .and_then(|params| params.get_mut("arguments"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| ApiError::bad("tools/call arguments must be an object"))?;
        let bound_group = connector["group_id"].as_str().unwrap_or("");
        if arguments
            .get("group_id")
            .and_then(Value::as_str)
            .is_some_and(|group_id| group_id != bound_group)
        {
            return Err(ApiError::forbidden("connector cannot access another group"));
        }
        arguments.insert("group_id".into(), Value::String(bound_group.into()));
        arguments
            .entry("actor_id")
            .or_insert_with(|| connector["actor_id"].clone());
    }
    Ok(Json(cccc_mcp::handle_request(&state.home, &request).await))
}

async fn mcp_options() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn mcp_options_token() -> StatusCode {
    StatusCode::NO_CONTENT
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn info_payload(connector: &Value) -> Value {
    json!({
        "name":"cccc-web-model-mcp",
        "version":env!("CARGO_PKG_VERSION"),
        "connector_id":connector["connector_id"],
        "group_id":connector["group_id"],
        "actor_id":connector["actor_id"]
    })
}

pub(super) fn find_authorized(
    state: &AppState,
    connector_id: &str,
    secret: Option<&str>,
) -> Result<Value, ApiError> {
    let item = load(state)?
        .into_iter()
        .find(|item| item["connector_id"] == connector_id)
        .ok_or_else(|| ApiError::not_found("web-model connector not found"))?;
    if item["revoked"].as_bool().unwrap_or(false) {
        return Err(ApiError::forbidden("web-model connector is revoked"));
    }
    if let Some(secret) = secret
        && item["secret"].as_str() != Some(secret)
    {
        return Err(ApiError::forbidden("invalid web-model connector secret"));
    }
    Ok(item)
}

pub(super) fn for_actor(state: &AppState, group_id: &str, actor_id: &str) -> Option<Value> {
    load(state).ok()?.into_iter().find(|item| {
        item["group_id"] == group_id
            && item["actor_id"] == actor_id
            && !item["revoked"].as_bool().unwrap_or(false)
    })
}

fn load(state: &AppState) -> Result<Vec<Value>, ApiError> {
    Ok(integration_state::global_get(&state.home, STORE_KEY)
        .map_err(io_error)?
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn public(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    let secret = result
        .remove("secret")
        .and_then(|value| value.as_str().map(str::to_owned));
    let id = item["connector_id"].as_str().unwrap_or("");
    result.insert("secret_available".into(), Value::Bool(secret.is_some()));
    result.insert(
        "secret_preview".into(),
        Value::String(secret.as_deref().map_or(String::new(), |value| {
            format!("...{}", &value[value.len().saturating_sub(6)..])
        })),
    );
    result.insert(
        "connector_url".into(),
        Value::String(format!("/mcp/web-model/{id}")),
    );
    result.insert(
        "connector_url_path_token".into(),
        Value::String(format!(
            "/mcp/web-model/{id}/token/{}",
            secret.unwrap_or_default()
        )),
    );
    Value::Object(result)
}

fn ensure_array(value: &mut Value) -> &mut Vec<Value> {
    if !value.is_array() {
        *value = Value::Array(Vec::new());
    }
    value.as_array_mut().expect("array initialized")
}

fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}

fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
