use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::utc_now;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;

use super::group_bridge_store::{BridgeStore, items, items_mut, short_id};
use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::auth::Principal;

#[derive(Debug, Default, Deserialize)]
struct GroupQuery {
    #[serde(default)]
    group_id: String,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusQuery {
    request_id: String,
    #[serde(default)]
    invite_id: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/group-bridge/status", get(status))
        .route("/api/group-bridge/unregister", post(unregister))
        .route(
            "/api/group-bridge/registrations/{registration_id}/deliveries/{idempotency_key}",
            get(delivery_status),
        )
        .route(
            "/api/group-bridge/pairing/requests/remote",
            post(remote_request),
        )
        .route(
            "/api/group-bridge/pairing/requests/remote/status",
            get(remote_request_status),
        )
        .route(
            "/api/group-bridge/pairing/requests/remote/claim",
            post(remote_claim),
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

async fn remote_request(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let pairing_code = required(&body, "pairing_code")?;
    let invite_id = required(&body, "invite_id")?;
    let requester_group_id = required(&body, "requester_group_id")?;
    let requester_peer_id = required(&body, "requester_peer_id")?;
    let request = BridgeStore::new(&state.home)
        .update(|value| {
            let invite = items_mut(value, "invites")
                .iter_mut()
                .find(|item| item["invite_id"] == invite_id && item["pairing_code"] == pairing_code)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "pairing invite not found")
                })?;
            if invite["status"] != "pending" {
                return Err(io::Error::other("pairing invite is not pending"));
            }
            let request_id = format!("preq_{}", short_id());
            let group_id = invite["group_id"].clone();
            invite["status"] = json!("requested");
            invite["request_id"] = json!(request_id.clone());
            invite["updated_at"] = json!(utc_now());
            let request = json!({
                "request_id":request_id,"invite_id":invite_id,"group_id":group_id,
                "remote_group_id":requester_group_id,"remote_peer_id":requester_peer_id,
                "requester_group_id":requester_group_id,
                "requester_group_title":body["requester_group_title"],
                "requester_endpoint":body["requester_endpoint"],
                "requester_node_id":body["requester_node_id"],
                "transport":"group_bridge_session","status":"pending",
                "created_at":utc_now(),"updated_at":utc_now()
            });
            items_mut(value, "requests").push(request.clone());
            Ok(request)
        })
        .map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(json!({"request":request})))
}

async fn remote_request_status(
    State(state): State<AppState>,
    Query(query): Query<RemoteStatusQuery>,
) -> ApiResult {
    let value = BridgeStore::new(&state.home).load().map_err(io_error)?;
    let request = items(&value, "requests")
        .iter()
        .find(|item| {
            item["request_id"] == query.request_id
                && (query.invite_id.is_empty() || item["invite_id"] == query.invite_id)
        })
        .cloned()
        .ok_or_else(|| ApiError::not_found("pairing request not found"))?;
    Ok(success(json!({"request":public_request(&request)})))
}

async fn remote_claim(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let request_id = required(&body, "request_id")?;
    let invite_id = required(&body, "invite_id")?;
    let pairing_code = required(&body, "pairing_code")?;
    let result = BridgeStore::new(&state.home)
        .update(|value| {
            let invite = value
                .get("invites")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .find(|item| item["invite_id"] == invite_id && item["pairing_code"] == pairing_code)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "pairing invite not found")
                })?;
            let request = value
                .get("requests")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .find(|item| item["request_id"] == request_id && item["invite_id"] == invite_id)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "pairing request not found")
                })?;
            if request["status"] != "approved" {
                return Err(io::Error::other("pairing request is not approved"));
            }
            let registration_id = request["registration_id"]
                .as_str()
                .ok_or_else(|| io::Error::other("approved request has no registration"))?;
            let registration = value
                .get("registrations")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .find(|item| item["registration_id"] == registration_id)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "registration not found"))?;
            let trust = value
                .get("trusts")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .find(|item| item["registration_id"] == registration_id)
                .cloned()
                .unwrap_or_else(|| json!({}));
            Ok(json!({
                "registration_id":registration_id,
                "credential":registration["credential"],
                "remote_group_id":invite["group_id"],
                "remote_group_title":request["group_id"],
                "remote_peer_id":registration["remote_peer_id"],
                "access_level":trust["access_level"].as_str().unwrap_or("messages")
            }))
        })
        .map_err(|error| ApiError::forbidden(error.to_string()))?;
    Ok(success(json!({"claim":result})))
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

fn public_request(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    result.remove("credential");
    Value::Object(result)
}

fn io_error(error: io::Error) -> ApiError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        ApiError::forbidden(error.to_string())
    } else {
        ApiError::bad(error.to_string())
    }
}
