use cccc_contracts::{ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, web_model_connectors};
use serde_json::Value;
use std::io;

use crate::AppState;
use crate::api::ApiError;

pub(super) fn load(state: &AppState) -> Result<Vec<Value>, ApiError> {
    web_model_connectors::load(&state.home).map_err(io_error)
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
    Ok(item)
}

pub(super) fn for_actor(state: &AppState, group_id: &str, actor_id: &str) -> Option<Value> {
    let group = GroupStore::new(state.home.clone())
        .ok()?
        .load(group_id)
        .ok()?;
    let actor = group.actors.iter().find(|actor| actor.id == actor_id)?;
    let connector = load(state).ok()?.into_iter().next()?;
    let mut binding = web_model_connectors::binding_for_actor(
        &connector,
        group_id,
        actor_id,
        &cccc_core::actors::generation_identity(actor),
    )?;
    binding["connector_id"] = connector["connector_id"].clone();
    binding["provider"] = connector["provider"].clone();
    Some(binding)
}

pub(super) fn resolve(
    state: &AppState,
    connector: &Value,
    request: &Value,
) -> Result<Value, ApiError> {
    let session = web_model_connectors::session_key(connector, &request["params"]["_meta"])
        .map_err(io_error)?;
    let mut binding =
        web_model_connectors::binding_for_session(connector, &session).ok_or_else(|| {
            ApiError::forbidden(
                "conversation_not_paired: pair this conversation in the Actor settings",
            )
        })?;
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(binding["group_id"].as_str().unwrap_or_default())
        .map_err(io_error)?;
    let actor = group
        .actors
        .iter()
        .find(|a| Some(a.id.as_str()) == binding["actor_id"].as_str())
        .ok_or_else(|| ApiError::forbidden("paired Actor is unavailable"))?;
    if actor.runtime != ActorRuntime::WebModel
        || actor.runner != RunnerKind::Headless
        || !actor.enabled
        || binding["generation"] != cccc_core::actors::generation_identity(actor)
    {
        return Err(ApiError::forbidden(
            "paired Actor is stopped or its identity changed",
        ));
    }
    if request["params"]["arguments"]
        .get("group_id")
        .and_then(Value::as_str)
        .is_some_and(|g| binding["group_id"] != g)
    {
        return Err(ApiError::forbidden("connector cannot access another group"));
    }
    binding["connector_id"] = connector["connector_id"].clone();
    Ok(binding)
}

pub(super) fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
