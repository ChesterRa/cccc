use axum::Json;
use axum::extract::{Path, State};
use cccc_core::settings;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

use super::web_model_connector_store as store;

pub(super) async fn list(State(state): State<AppState>) -> ApiResult {
    let mut connectors = store::load(&state)?;
    connectors.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    let base_url = connector_base_url(&state)?;
    Ok(success(json!({
        "requires_reconfiguration":cccc_core::web_model_connectors::requires_reconfiguration(&state.home).map_err(store::io_error)?,
        "connectors": connectors.iter().map(|item| public(item, &base_url)).collect::<Vec<_>>(),
        // The configured address used to build connector URLs. This metadata
        // does not verify connectivity from an external service.
        "connector_base_url": base_url
    })))
}

pub(super) async fn create(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let provider = match body.get("provider") {
        None => "chatgpt_web",
        Some(value) => value
            .as_str()
            .ok_or_else(|| ApiError::bad("provider must be a string"))?,
    };
    // create_only: create when absent, never rotate an existing credential. Only
    // a real boolean may choose; a mistyped flag must not fall back to rotation.
    let create_only = match body.get("create_only") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(ApiError::bad("create_only must be a boolean")),
    };
    let result = super::web_model_delivery_completion::call(
        &state,
        "web_model_connector_configure",
        json!({"by":"user","provider":provider,"create_only":create_only})
            .as_object()
            .expect("literal JSON object")
            .clone(),
    )
    .await?;
    let base_url = connector_base_url(&state)?;
    let mut connector = public(&result["connector"], &base_url);
    let url = connector["connector_url"].as_str().unwrap_or_default();
    let secret = result["secret"].as_str().unwrap_or_default();
    connector["connector_url_path_token"] = json!(format!("{url}/token/{secret}"));
    connector["secret_available"] = json!(true);
    Ok(success(json!({"connector":connector,"secret":secret})))
}

fn connector_base_url(state: &AppState) -> Result<String, ApiError> {
    let settings = settings::load(&state.home).map_err(store::io_error)?;
    let mut base = settings
        .remote_access
        .get("web_public_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_owned();
    if base.ends_with("/ui") {
        base.truncate(base.len() - "/ui".len());
    }
    Ok(base)
}

fn public(item: &Value, base_url: &str) -> Value {
    let id = item["connector_id"].as_str().unwrap_or_default();
    let mut result = json!({"connector_id":id,"kind":"web_model_connector","routing_mode":item["routing_mode"],"provider":item["provider"],
        "connector_url":format!("{base_url}/mcp/web-model/{id}"),"secret_available":false,
        "bound_actor_count":item["bindings"].as_object().map_or(0, |b| b.len())});
    for field in [
        "created_at",
        "updated_at",
        "revoked",
        "secret_preview",
        "last_activity_at",
        "last_method",
        "last_tool_name",
        "last_call_status",
        "last_error",
        "events_log",
    ] {
        result[field] = item[field].clone();
    }
    // Which Actor the Dot is bound to; its credentials and callback stay private.
    let dots = &item["dots"];
    result["dots_actor"] = if dots["actor_id"].is_string() {
        json!({"group_id":dots["group_id"],"actor_id":dots["actor_id"]})
    } else {
        Value::Null
    };
    result
}

pub(super) async fn revoke(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
) -> ApiResult {
    let result = super::web_model_delivery_completion::call(
        &state,
        "web_model_connector_revoke",
        json!({"by":"user","connector_id":connector_id})
            .as_object()
            .expect("literal JSON object")
            .clone(),
    )
    .await?;
    Ok(success(result))
}
