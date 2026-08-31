use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;

use super::group_bridge_store::{BridgeStore, items, items_mut};
use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::auth::Principal;

#[derive(Debug, Default, Deserialize)]
struct GroupQuery {
    #[serde(default)]
    group_id: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/group-bridge/status", get(status))
        .route("/api/group-bridge/unregister", post(unregister))
        .route(
            "/api/group-bridge/registrations/{registration_id}/deliveries/{idempotency_key}",
            get(delivery_status),
        )
}

async fn status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    if !query.group_id.is_empty() {
        ensure_access(&principal, &query.group_id)?;
    }
    let value = BridgeStore::new(&state.home).load().map_err(io_error)?;
    let registrations = items(&value, "registrations")
        .iter()
        .filter(|item| {
            let group_id = item["group_id"].as_str().unwrap_or("");
            (query.group_id.is_empty() || group_id == query.group_id) && principal.allows(group_id)
        })
        .map(public_registration)
        .collect::<Vec<_>>();
    Ok(success(json!({"registrations":registrations})))
}

async fn unregister(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let registration_id = required(&body, "registration_id")?;
    let store = BridgeStore::new(&state.home);
    let deleted = store
        .update(|value| {
            let registrations = items_mut(value, "registrations");
            let item = registrations
                .iter()
                .find(|item| item["registration_id"] == registration_id)
                .cloned();
            let Some(item) = item else { return Ok(false) };
            if !principal.allows(item["group_id"].as_str().unwrap_or("")) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "group access denied",
                ));
            }
            registrations.retain(|item| item["registration_id"] != registration_id);
            Ok(true)
        })
        .map_err(io_error)?;
    if !deleted {
        return Err(ApiError::not_found("registration not found"));
    }
    Ok(success(json!({"deleted":true})))
}

async fn delivery_status(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((registration_id, idempotency_key)): Path<(String, String)>,
) -> ApiResult {
    let value = BridgeStore::new(&state.home).load().map_err(io_error)?;
    let registration = items(&value, "registrations")
        .iter()
        .find(|item| item["registration_id"] == registration_id)
        .ok_or_else(|| ApiError::not_found("registration not found"))?;
    ensure_access(&principal, registration["group_id"].as_str().unwrap_or(""))?;
    let receipt = items(&value, "deliveries")
        .iter()
        .find(|item| {
            item["registration_id"] == registration_id && item["idempotency_key"] == idempotency_key
        })
        .cloned();
    Ok(success(json!({"receipt":receipt})))
}

pub(super) fn ensure_access(principal: &Principal, group_id: &str) -> Result<(), ApiError> {
    principal
        .allows(group_id)
        .then_some(())
        .ok_or_else(|| ApiError::forbidden("group access denied"))
}

pub(super) fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}

fn public_registration(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    result.remove("credential");
    result.remove("secret");
    Value::Object(result)
}

fn io_error(error: io::Error) -> ApiError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        ApiError::forbidden(error.to_string())
    } else {
        ApiError::bad(error.to_string())
    }
}
