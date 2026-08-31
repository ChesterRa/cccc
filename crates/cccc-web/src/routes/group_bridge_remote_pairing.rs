use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::utc_now;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;

use super::group_bridge::required;
use super::group_bridge_pairing_endpoint::normalize_endpoint;
use super::group_bridge_pairing_policy::{consume_pending_invite, timestamp_not_live};
use super::group_bridge_store::{BridgeStore, items, items_mut, short_id};
use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

#[derive(Debug, Deserialize)]
struct RemoteStatusQuery {
    request_id: String,
    #[serde(default)]
    invite_id: String,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
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

async fn remote_request(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let pairing_code = required(&body, "pairing_code")?;
    let invite_id = required(&body, "invite_id")?;
    let requester_group_id = required(&body, "requester_group_id")?;
    let requester_peer_id = required(&body, "requester_peer_id")?;
    let requester_endpoint = body["requester_endpoint"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_endpoint)
        .transpose()?
        .unwrap_or_default();
    let request = BridgeStore::new(&state.home)
        .update(|value| {
            let invite = items_mut(value, "invites")
                .iter_mut()
                .find(|item| item["invite_id"] == invite_id && item["pairing_code"] == pairing_code)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "pairing invite not found")
                })?;
            if !consume_pending_invite(invite)? {
                return Ok(None);
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
                "requester_endpoint":requester_endpoint,
                "requester_node_id":body["requester_node_id"],
                "transport":"group_bridge_session","status":"pending",
                "created_at":utc_now(),"updated_at":utc_now()
            });
            items_mut(value, "requests").push(request.clone());
            Ok(Some(request))
        })
        .map_err(|error| ApiError::bad(error.to_string()))?;
    request
        .map(|request| success(json!({"request":request})))
        .ok_or_else(|| ApiError::bad("pairing invite expired"))
}

async fn remote_request_status(
    State(state): State<AppState>,
    Query(query): Query<RemoteStatusQuery>,
) -> ApiResult {
    if query.invite_id.trim().is_empty() {
        return Err(ApiError::not_found("pairing request not found"));
    }
    let value = BridgeStore::new(&state.home).load().map_err(io_error)?;
    let request = items(&value, "requests")
        .iter()
        .find(|item| item["request_id"] == query.request_id && item["invite_id"] == query.invite_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("pairing request not found"))?;
    Ok(success(json!({"request":public_request(&request)})))
}

async fn remote_claim(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
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
                .cloned()
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "pairing invite not found")
                })?;
            let request_index = value
                .get("requests")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .position(|item| item["request_id"] == request_id && item["invite_id"] == invite_id)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "pairing request not found")
                })?;
            let request = value["requests"][request_index].clone();
            if request["status"] != "approved" {
                return Err(io::Error::other("pairing request is not approved"));
            }
            if timestamp_not_live(&request["claim_expires_at"]) {
                return Err(io::Error::other("pairing credential claim expired"));
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
                .find(|item| {
                    item["registration_id"] == registration_id && item["status"] == "active"
                })
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "registration not found"))?;
            let trust = value
                .get("trusts")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .find(|item| {
                    item["registration_id"] == registration_id && item["status"] == "active"
                })
                .cloned()
                .ok_or_else(|| io::Error::other("pairing trust is not active"))?;
            if request["claimed_at"].is_null() {
                value["requests"][request_index]["claimed_at"] = json!(utc_now());
            }
            value["requests"][request_index]["updated_at"] = json!(utc_now());
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
    Ok((
        [
            (header::CACHE_CONTROL, "no-store, private"),
            (header::PRAGMA, "no-cache"),
        ],
        success(json!({"claim":result})),
    )
        .into_response())
}

fn public_request(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    result.remove("credential");
    Value::Object(result)
}

fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
