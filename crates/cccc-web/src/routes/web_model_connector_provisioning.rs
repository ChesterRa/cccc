use axum::Json;
use axum::extract::{Path, State};
use cccc_contracts::{ActorRuntime, utc_now};
use cccc_core::GroupStore;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

use super::web_model_connector_store as store;
use super::web_model_connectors::required;

pub(super) async fn list(State(state): State<AppState>) -> ApiResult {
    let mut connectors = store::load(&state)?;
    connectors.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    Ok(success(json!({
        "connectors": connectors.iter().map(public).collect::<Vec<_>>()
    })))
}

pub(super) async fn create(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    let group = GroupStore::new(state.home.clone())
        .map_err(store::io_error)?
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
    let replaced = store::replace_active(&state, &connector)?;
    Ok(success(json!({
        "connector": public(&connector),
        "secret": secret,
        "replaced_connector_ids": replaced
    })))
}

fn public(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    let secret = result
        .remove("secret")
        .and_then(|value| value.as_str().map(str::to_owned));
    let id = item["connector_id"].as_str().unwrap_or("");
    let secret_value = secret.as_deref().unwrap_or_default();
    result.insert("secret_available".into(), Value::Bool(secret.is_some()));
    result.insert(
        "secret_preview".into(),
        Value::String(secret.as_deref().map_or(String::new(), |value| {
            format!("...{}", &value[value.len().saturating_sub(6)..])
        })),
    );
    result.insert(
        "connector_url".into(),
        json!(format!("/mcp/web-model/{id}")),
    );
    result.insert(
        "connector_url_path_token".into(),
        json!(format!("/mcp/web-model/{id}/token/{secret_value}")),
    );
    result.insert(
        "connector_url_with_token".into(),
        json!(format!("/mcp/web-model/{id}?token={secret_value}")),
    );
    Value::Object(result)
}

pub(super) async fn revoke(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
) -> ApiResult {
    if !store::revoke(&state, &connector_id)? {
        return Err(ApiError::not_found("web-model connector not found"));
    }
    Ok(success(json!({"revoked":true,"connector_id":connector_id})))
}
