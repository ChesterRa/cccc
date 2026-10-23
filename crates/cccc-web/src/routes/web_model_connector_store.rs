use cccc_contracts::RunnerKind;
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
    let provider = actor.runtime.web_model_provider()?;
    let connector = load(state)
        .ok()?
        .into_iter()
        .find(|c| c["provider"] == provider)?;
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
    let mut binding = if connector["provider"] == "grok_web" {
        let token = request["params"]["arguments"]["actor_token"]
            .as_str()
            .unwrap_or_default();
        web_model_connectors::grok_webhook::resolve(connector, token, request["params"]["name"].as_str().unwrap_or("")).ok_or_else(|| {
            ApiError::forbidden("actor_token_invalid: Use this Bot's current Webhook event claim credential. Native business tools need the execution credential returned by the current claim. Never recover credentials from chat history")
        })?
    } else if request["params"]["arguments"].get("actor_token").is_some() {
        let token = request["params"]["arguments"]["actor_token"]
            .as_str()
            .unwrap_or_default();
        web_model_connectors::dots_binding_for_token(
            connector,
            token,
            request["params"]["name"].as_str().unwrap_or_default(),
        )
        .ok_or_else(|| {
            ApiError::forbidden(
                "dots_token_invalid: Dots claims work with {} from its explicitly paired conversation. Update the existing native task using Get setup instructions in CCCC. Business tools require the execution credential returned by the current claim",
            )
        })?
    } else {
        let session = web_model_connectors::session_key(connector, &request["params"]["_meta"])
            .map_err(io_error)?;
        let paired = web_model_connectors::binding_for_session(connector, &session).ok_or_else(|| {
            ApiError::forbidden(
                "conversation_not_paired: use the selected Actor's setup instructions to explicitly pair this conversation; missing or changed host identity cannot select an Actor",
            )
        })?;
        if paired["transport"] == "mcp_events" {
            if request["params"]["name"] != "cccc_runtime_wait_next_turn" {
                return Err(ApiError::forbidden(
                    "dots_execution_token_required: pairing authorizes only one current task claim. Business tools require the execution actor_token returned by cccc_runtime_wait_next_turn",
                ));
            }
            web_model_connectors::dots_claim_binding(connector, &session).ok_or_else(|| {
                ApiError::forbidden("dots_pairing_changed: get new setup instructions in CCCC")
            })?
        } else {
            paired
        }
    };
    let group = GroupStore::new(state.home.clone())
        .map_err(io_error)?
        .load(binding["group_id"].as_str().unwrap_or_default())
        .map_err(io_error)?;
    let actor = group
        .actors
        .iter()
        .find(|a| Some(a.id.as_str()) == binding["actor_id"].as_str())
        .ok_or_else(|| {
            ApiError::forbidden("paired_actor_unavailable: The paired Actor no longer exists")
        })?;
    if binding["generation"] != cccc_core::actors::generation_identity(actor) {
        return Err(ApiError::forbidden(
            "paired_actor_identity_changed: The paired Actor was replaced; explicitly reconnect the intended Actor",
        ));
    }
    if actor.runtime.web_model_provider() != connector["provider"].as_str()
        || actor.runner != RunnerKind::Headless
    {
        return Err(ApiError::forbidden(
            "paired_actor_runtime_changed: The paired Actor's runtime changed; review its connection settings",
        ));
    }
    if !actor.enabled {
        return Err(ApiError::forbidden(
            "paired_actor_stopped: CCCC access is paused. Pairing is retained; resume access in CCCC to receive work. Do not pair again",
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
