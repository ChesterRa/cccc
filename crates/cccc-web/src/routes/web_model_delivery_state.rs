use cccc_core::{GroupStore, integration_state};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::ApiError;

use super::web_model_browser::TARGETS_KEY;
use super::web_model_connector_activity::{self as activity, Activity};
use super::web_model_connector_store;

pub(super) fn target(state: &AppState, group_id: &str, actor_id: &str) -> Result<Value, ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    Ok(integration_state::group_get(&store, group_id, TARGETS_KEY)
        .map_err(io_error)?
        .get(actor_id)
        .cloned()
        .unwrap_or_else(|| json!({})))
}

pub(super) fn update_target(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    patch: Value,
) -> Result<(), ApiError> {
    let store = GroupStore::new(state.home.clone()).map_err(io_error)?;
    integration_state::group_update(&store, group_id, TARGETS_KEY, |value| {
        let targets = value.as_object_mut().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid browser target store",
            )
        })?;
        let target = targets.entry(actor_id).or_insert_with(|| json!({}));
        let target = target.as_object_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid browser target")
        })?;
        target.extend(patch.as_object().cloned().unwrap_or_default());
        Ok(())
    })
    .map_err(io_error)
}

pub(super) fn record_connector(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    status: &str,
    turn_id: &str,
    error: &str,
) -> Result<(), ApiError> {
    let Some(connector) = web_model_connector_store::for_actor(state, group_id, actor_id) else {
        return Ok(());
    };
    activity::record(
        state,
        connector["connector_id"].as_str().unwrap_or(""),
        Activity {
            method: "browser/delivery",
            tool_name: "",
            call_status: status,
            wait_status: status,
            turn_id,
            error,
        },
    )
}

fn io_error(error: std::io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
