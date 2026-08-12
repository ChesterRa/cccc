use cccc_contracts::{ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, web_model_connectors};
use serde_json::Value;
use std::io;

use crate::AppState;
use crate::api::ApiError;

pub(super) fn load(state: &AppState) -> Result<Vec<Value>, ApiError> {
    web_model_connectors::load(&state.home).map_err(io_error)
}

pub(super) fn replace_active(state: &AppState, connector: &Value) -> Result<Vec<String>, ApiError> {
    web_model_connectors::replace_active(&state.home, connector).map_err(io_error)
}

pub(super) fn revoke(state: &AppState, connector_id: &str) -> Result<bool, ApiError> {
    web_model_connectors::revoke(&state.home, connector_id).map_err(io_error)
}

pub(super) fn update_connector(
    state: &AppState,
    connector_id: &str,
    change: impl FnOnce(&mut Value),
) -> Result<bool, ApiError> {
    web_model_connectors::update_connector(&state.home, connector_id, change).map_err(io_error)
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
    if secret.is_some_and(|secret| !web_model_connectors::secret_matches(&item, secret)) {
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

pub(super) fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
