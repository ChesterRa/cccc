use std::io;

use cccc_contracts::{ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, integration_state};
use serde_json::Value;

use crate::AppState;
use crate::api::ApiError;

pub(super) const STORE_KEY: &str = "web_model_connectors";

pub(super) fn load(state: &AppState) -> Result<Vec<Value>, ApiError> {
    Ok(integration_state::global_get(&state.home, STORE_KEY)
        .map_err(io_error)?
        .as_array()
        .cloned()
        .unwrap_or_default())
}

pub(super) fn replace_active(state: &AppState, connector: &Value) -> Result<Vec<String>, ApiError> {
    integration_state::global_update(&state.home, STORE_KEY, |value| {
        let items = ensure_array(value);
        let mut replaced = Vec::new();
        items.retain(|item| {
            let same = item["group_id"] == connector["group_id"]
                && item["actor_id"] == connector["actor_id"]
                && !item["revoked"].as_bool().unwrap_or(false);
            if same && let Some(id) = item["connector_id"].as_str() {
                replaced.push(id.to_owned());
            }
            !same
        });
        items.push(connector.clone());
        Ok(replaced)
    })
    .map_err(io_error)
}

pub(super) fn revoke(state: &AppState, connector_id: &str) -> Result<bool, ApiError> {
    integration_state::global_update(&state.home, STORE_KEY, |value| {
        let mut found = false;
        for item in ensure_array(value) {
            if item["connector_id"] == connector_id {
                item["revoked"] = Value::Bool(true);
                item["updated_at"] = Value::String(cccc_contracts::utc_now());
                found = true;
            }
        }
        Ok(found)
    })
    .map_err(io_error)
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
    let group_id = item["group_id"].as_str().unwrap_or("");
    let actor_id = item["actor_id"].as_str().unwrap_or("");
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(group_id)
        .map_err(|_| ApiError::forbidden("web-model connector group is unavailable"))?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| ApiError::forbidden("web-model connector actor is unavailable"))?;
    if actor.runtime != ActorRuntime::WebModel
        || actor.runner != RunnerKind::Headless
        || !actor.enabled
    {
        return Err(ApiError::forbidden(
            "web-model connector actor is stopped or no longer eligible",
        ));
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

pub(super) fn ensure_array(value: &mut Value) -> &mut Vec<Value> {
    if !value.is_array() {
        *value = Value::Array(Vec::new());
    }
    value.as_array_mut().expect("array initialized")
}

pub(super) fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
