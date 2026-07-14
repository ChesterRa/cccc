use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::utc_now;
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io;
use uuid::Uuid;

use super::group_bridge::{ensure_access, required};
use super::group_bridge_store::{BridgeStore, items, items_mut, short_id};
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
        .route("/api/group-bridge/pairing/identity", get(identity))
        .route("/api/group-bridge/pairing/invites", post(create_invite))
        .route(
            "/api/group-bridge/pairing/connection-info",
            post(connection_info),
        )
        .route(
            "/api/group-bridge/pairing/remote-requests",
            post(remote_submit),
        )
        .route(
            "/api/group-bridge/pairing/requests",
            get(list_requests).post(create_request),
        )
        .route(
            "/api/group-bridge/pairing/requests/{request_id}/approve",
            post(approve),
        )
        .route(
            "/api/group-bridge/pairing/requests/{request_id}/reject",
            post(reject),
        )
        .route("/api/group-bridge/pairing/trusts", get(list_trusts))
        .route(
            "/api/group-bridge/pairing/trusts/{trust_id}/revoke",
            post(revoke_trust),
        )
        .route(
            "/api/group-bridge/pairing/trusts/{trust_id}/access",
            post(update_access),
        )
        .route(
            "/api/group-bridge/pairing/trusts/{trust_id}/refresh",
            post(refresh_trust),
        )
        .route("/api/group-bridge/pairing/outbounds", get(list_outbounds))
        .route(
            "/api/group-bridge/pairing/outbounds/{outbound_id}/sync",
            post(sync_outbound),
        )
        .route(
            "/api/group-bridge/pairing/outbounds/{outbound_id}/delete",
            post(delete_outbound),
        )
}

async fn identity(State(state): State<AppState>) -> ApiResult {
    Ok(success(json!({
        "identity":BridgeStore::new(&state.home).identity().map_err(io_error)?
    })))
}

async fn create_invite(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    ensure_access(&principal, &group_id)?;
    let ttl = body["ttl_seconds"]
        .as_i64()
        .unwrap_or(600)
        .clamp(60, 86_400);
    let code = pairing_code();
    let invite = json!({
        "invite_id":format!("pinv_{}",short_id()),"group_id":group_id,
        "remote_group_id":body["remote_group_id"],"remote_peer_id":body["remote_peer_id"],
        "transport":"group_bridge_session","status":"pending","pairing_code":code,
        "created_at":utc_now(),"updated_at":utc_now(),
        "expires_at":(Utc::now()+Duration::seconds(ttl)).to_rfc3339(),"request_id":""
    });
    BridgeStore::new(&state.home)
        .update(|value| {
            items_mut(value, "invites").push(invite.clone());
            Ok(())
        })
        .map_err(io_error)?;
    Ok(success(json!({"invite":invite})))
}

async fn connection_info(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let invite_id = required(&body, "invite_id")?;
    ensure_access(&principal, &group_id)?;
    let store = BridgeStore::new(&state.home);
    let value = store.load().map_err(io_error)?;
    let invite = items(&value, "invites")
        .iter()
        .find(|item| item["invite_id"] == invite_id && item["group_id"] == group_id)
        .ok_or_else(|| ApiError::not_found("pairing invite not found"))?;
    let endpoint = normalize_endpoint(body["issuer_endpoint"].as_str().unwrap_or(""))?;
    let identity = store.identity().map_err(io_error)?;
    Ok(success(json!({"payload":{
        "type":"cccc.group_bridge_session.connection_info","version":2,
        "issuer_endpoint":endpoint,"issuer_group_id":group_id,
        "issuer_group_title":body["issuer_group_title"],
        "issuer_peer_id":identity["peer_id"],"issuer_node_id":identity["node_id"],
        "code":invite["pairing_code"],"expires_at":invite["expires_at"],
        "nonce":invite_id
    }})))
}

async fn create_request(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let requester_group_id = required(&body, "requester_group_id")?;
    let requester_peer_id = required(&body, "requester_peer_id")?;
    let code = required(&body, "pairing_code")?;
    ensure_access(&principal, &requester_group_id)?;
    let request = BridgeStore::new(&state.home)
        .update(|value| {
            let invite = items_mut(value, "invites")
                .iter_mut()
                .find(|item| {
                    item["pairing_code"] == code
                        && body["invite_id"]
                            .as_str()
                            .is_none_or(|id| id.is_empty() || item["invite_id"] == id)
                })
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "pairing invite not found")
                })?;
            let request_id = format!("preq_{}", short_id());
            let request = json!({
                "request_id":request_id,"invite_id":invite["invite_id"],
                "group_id":invite["group_id"],"remote_group_id":requester_group_id,
                "remote_peer_id":requester_peer_id,"transport":"group_bridge_session",
                "status":"pending","created_at":utc_now(),"updated_at":utc_now()
            });
            invite["status"] = json!("requested");
            invite["request_id"] = json!(request_id);
            items_mut(value, "requests").push(request.clone());
            Ok(request)
        })
        .map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(json!({"request":request})))
}

async fn list_requests(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    Ok(success(json!({"requests":filtered(
        &BridgeStore::new(&state.home).load().map_err(io_error)?,
        "requests",&query.group_id,&principal,"group_id"
    )?})))
}

async fn approve(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let result = BridgeStore::new(&state.home)
        .update(|value| {
            let request = items_mut(value, "requests")
                .iter_mut()
                .find(|item| item["request_id"] == request_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pairing request not found"))?;
            let group_id = request["group_id"].as_str().unwrap_or("");
            if !principal.allows(group_id) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied,"group access denied"));
            }
            if request["status"] != "pending" {
                return Err(io::Error::other("pairing request is not pending"));
            }
            let registration_id = format!("greg_{}", short_id());
            let trust_id = format!("ptrust_{}", short_id());
            let secret = format!("{}{}",Uuid::new_v4().simple(),Uuid::new_v4().simple());
            request["status"] = json!("approved");
            request["approved_by"] = body.get("approver_user_id").cloned().unwrap_or(json!(""));
            request["registration_id"] = json!(registration_id.clone());
            request["updated_at"] = json!(utc_now());
            let approved = request.clone();
            let registration = json!({
                "registration_id":registration_id,"group_id":approved["group_id"],
                "url":approved["requester_endpoint"],"transport":"group_bridge_session",
                "remote_group_id":approved["remote_group_id"],"remote_peer_id":approved["remote_peer_id"],
                "credential":secret,"user_id":body["approver_user_id"],"status":"active",
                "created_at":utc_now(),"updated_at":utc_now()
            });
            let trust = json!({
                "trust_id":trust_id,"request_id":request_id,"registration_id":registration_id,
                "group_id":approved["group_id"],"remote_group_id":approved["remote_group_id"],
                "remote_group_title":approved["requester_group_title"],
                "remote_endpoint":approved["requester_endpoint"],"remote_peer_id":approved["remote_peer_id"],
                "transport":"group_bridge_session","status":"active","access_level":"messages",
                "remote_access_level":"messages","created_at":utc_now(),"updated_at":utc_now()
            });
            items_mut(value,"registrations").push(registration.clone());
            items_mut(value,"trusts").push(trust.clone());
            Ok((approved,public_registration(&registration),trust))
        })
        .map_err(state_error)?;
    Ok(success(
        json!({"request":result.0,"registration":result.1,"trust":result.2}),
    ))
}

async fn reject(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(request_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let request = mutate_owned(
        &state,
        "requests",
        "request_id",
        &request_id,
        &principal,
        |item| {
            item["status"] = json!("rejected");
            item["rejected_by"] = body.get("rejected_by").cloned().unwrap_or(json!(""));
            item["rejection_reason"] = body.get("reason").cloned().unwrap_or(json!(""));
        },
    )?;
    Ok(success(json!({"request":request})))
}

async fn list_trusts(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    let trusts = filtered(
        &BridgeStore::new(&state.home).load().map_err(io_error)?,
        "trusts",
        &query.group_id,
        &principal,
        "group_id",
    )?;
    Ok(success(
        json!({"trusts":trusts.iter().map(public_trust).collect::<Vec<_>>()}),
    ))
}

async fn revoke_trust(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(trust_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let trust = mutate_owned(
        &state,
        "trusts",
        "trust_id",
        &trust_id,
        &principal,
        |item| {
            item["status"] = json!("revoked");
            item["revoked_by"] = body.get("revoked_by").cloned().unwrap_or(json!(""));
        },
    )?;
    Ok(success(json!({"trust":trust})))
}

async fn update_access(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(trust_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let level = required(&body, "access_level")?;
    if !matches!(level.as_str(), "messages" | "read" | "full") {
        return Err(ApiError::bad(
            "access_level must be messages, read, or full",
        ));
    }
    let trust = mutate_owned(
        &state,
        "trusts",
        "trust_id",
        &trust_id,
        &principal,
        |item| {
            item["access_level"] = json!(level);
            item["access_updated_by"] = body.get("updated_by").cloned().unwrap_or(json!(""));
        },
    )?;
    Ok(success(json!({"trust":trust})))
}

async fn refresh_trust(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(trust_id): Path<String>,
) -> ApiResult {
    let trust = mutate_owned(&state, "trusts", "trust_id", &trust_id, &principal, |_| {})?;
    let remote_status =
        json!({"status":trust["status"],"access_level":trust["remote_access_level"]});
    Ok(success(
        json!({"trust":trust,"remote_status":remote_status}),
    ))
}

async fn list_outbounds(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<GroupQuery>,
) -> ApiResult {
    let outbounds = filtered(
        &BridgeStore::new(&state.home).load().map_err(io_error)?,
        "outbounds",
        &query.group_id,
        &principal,
        "local_group_id",
    )?;
    Ok(success(
        json!({"outbounds":outbounds.iter().map(public_outbound).collect::<Vec<_>>()}),
    ))
}

async fn remote_submit(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Value>,
) -> ApiResult {
    let local_group_id = required(&body, "local_group_id")?;
    ensure_access(&principal, &local_group_id)?;
    let payload = body
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::bad("payload is required"))?;
    let endpoint = normalize_endpoint(
        payload
            .get("issuer_endpoint")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )?;
    let code = payload
        .get("code")
        .or_else(|| payload.get("pairing_code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let invite_id = payload
        .get("nonce")
        .or_else(|| payload.get("invite_id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if code.is_empty() || invite_id.is_empty() {
        return Err(ApiError::bad("connection payload is incomplete"));
    }
    let identity = BridgeStore::new(&state.home).identity().map_err(io_error)?;
    let request_body = json!({
        "pairing_code":code,"invite_id":invite_id,"requester_group_id":local_group_id,
        "requester_group_title":body["local_group_title"],"requester_endpoint":"",
        "requester_peer_id":identity["peer_id"],"requester_node_id":identity["node_id"],"requester_multiaddrs":[]
    });
    let (remote_request, error) = post_remote(
        &endpoint,
        "/api/group-bridge/pairing/requests/remote",
        &request_body,
    )
    .await;
    let outbound = json!({
        "outbound_id":format!("pout_{}",short_id()),"local_group_id":local_group_id,
        "issuer_endpoint":endpoint,"issuer_group_id":payload.get("issuer_group_id").cloned().unwrap_or(json!("")),
        "issuer_group_title":payload.get("issuer_group_title").cloned().unwrap_or(json!("")),
        "issuer_peer_id":payload.get("issuer_peer_id").cloned().unwrap_or(json!("")),
        "invite_id":invite_id,"pairing_code":code,
        "status":if error.is_empty(){"submitted"}else{"failed"},
        "remote_request":remote_request,"last_error":error,"created_at":utc_now(),"updated_at":utc_now()
    });
    BridgeStore::new(&state.home)
        .update(|value| {
            items_mut(value, "outbounds").push(outbound.clone());
            Ok(())
        })
        .map_err(io_error)?;
    Ok(success(json!({"outbound":public_outbound(&outbound)})))
}

async fn sync_outbound(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(outbound_id): Path<String>,
) -> ApiResult {
    let current = find_owned(
        &state,
        "outbounds",
        "outbound_id",
        &outbound_id,
        &principal,
        "local_group_id",
    )?;
    let endpoint = normalize_endpoint(current["issuer_endpoint"].as_str().unwrap_or(""))?;
    let request_id = current["remote_request"]["request_id"]
        .as_str()
        .unwrap_or("");
    let invite_id = current["invite_id"].as_str().unwrap_or("");
    let path = format!(
        "/api/group-bridge/pairing/requests/remote/status?request_id={request_id}&invite_id={invite_id}"
    );
    let (remote, mut error) = get_remote(&endpoint, &path).await;
    let approved = remote["request"]["status"] == "approved";
    let (claim, claim_error) = if error.is_empty() && approved {
        post_remote(
            &endpoint,
            "/api/group-bridge/pairing/requests/remote/claim",
            &json!({
                "request_id":request_id,
                "invite_id":invite_id,
                "pairing_code":current["pairing_code"]
            }),
        )
        .await
    } else {
        (json!({}), String::new())
    };
    if !claim_error.is_empty() {
        error = claim_error;
    }
    let outbound = BridgeStore::new(&state.home)
        .update(|value| {
            let index = items_mut(value, "outbounds")
                .iter()
                .position(|item| item["outbound_id"] == outbound_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "outbound not found"))?;
            let mut item = items_mut(value, "outbounds")[index].clone();
            if !error.is_empty() {
                item["last_error"] = json!(error);
            } else if let Some(request) = remote.get("request") {
                item["remote_request"] = request.clone();
                item["status"] = request["status"].clone();
                item["last_error"] = json!("");
            }
            if let Some(claim) = claim.get("claim") {
                item["credential"] = claim["credential"].clone();
                item["status"] = json!("active");
                let trust_id = format!("ptrust_{}", short_id());
                let local_group_id = item["local_group_id"].clone();
                let remote_group_id = item["issuer_group_id"].clone();
                let existing = items_mut(value, "trusts").iter_mut().find(|trust| {
                    trust["group_id"] == local_group_id
                        && trust["remote_group_id"] == remote_group_id
                });
                let trust = json!({
                    "trust_id":trust_id,"group_id":local_group_id,
                    "remote_group_id":remote_group_id,
                    "remote_group_title":item["issuer_group_title"],
                    "remote_endpoint":item["issuer_endpoint"],
                    "remote_peer_id":item["issuer_peer_id"],
                    "registration_id":claim["registration_id"],
                    "credential":claim["credential"],
                    "transport":"group_bridge_session","status":"active",
                    "access_level":"messages","remote_access_level":claim["access_level"],
                    "created_at":utc_now(),"updated_at":utc_now()
                });
                if let Some(existing) = existing {
                    *existing = trust;
                } else {
                    items_mut(value, "trusts").push(trust);
                }
            }
            item["updated_at"] = json!(utc_now());
            items_mut(value, "outbounds")[index] = item.clone();
            Ok(item)
        })
        .map_err(state_error)?;
    Ok(success(json!({"outbound":public_outbound(&outbound)})))
}

async fn delete_outbound(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(outbound_id): Path<String>,
) -> ApiResult {
    let current = find_owned(
        &state,
        "outbounds",
        "outbound_id",
        &outbound_id,
        &principal,
        "local_group_id",
    )?;
    let _ = current;
    BridgeStore::new(&state.home)
        .update(|value| {
            items_mut(value, "outbounds").retain(|item| item["outbound_id"] != outbound_id);
            Ok(())
        })
        .map_err(io_error)?;
    Ok(success(json!({"deleted":true})))
}

fn filtered(
    value: &Value,
    section: &str,
    group_id: &str,
    principal: &Principal,
    field: &str,
) -> Result<Vec<Value>, ApiError> {
    if !group_id.is_empty() {
        ensure_access(principal, group_id)?;
    }
    Ok(items(value, section)
        .iter()
        .filter(|item| {
            let gid = item[field].as_str().unwrap_or("");
            (group_id.is_empty() || gid == group_id) && principal.allows(gid)
        })
        .cloned()
        .collect())
}

fn find_owned(
    state: &AppState,
    section: &str,
    id_field: &str,
    id: &str,
    principal: &Principal,
    group_field: &str,
) -> Result<Value, ApiError> {
    let value = BridgeStore::new(&state.home).load().map_err(io_error)?;
    let item = items(&value, section)
        .iter()
        .find(|item| item[id_field] == id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("group bridge record not found"))?;
    ensure_access(principal, item[group_field].as_str().unwrap_or(""))?;
    Ok(item)
}

fn mutate_owned(
    state: &AppState,
    section: &str,
    id_field: &str,
    id: &str,
    principal: &Principal,
    change: impl FnOnce(&mut Value),
) -> Result<Value, ApiError> {
    BridgeStore::new(&state.home)
        .update(|value| {
            let item = items_mut(value, section)
                .iter_mut()
                .find(|item| item[id_field] == id)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "group bridge record not found")
                })?;
            let group_field = if section == "outbounds" {
                "local_group_id"
            } else {
                "group_id"
            };
            if !principal.allows(item[group_field].as_str().unwrap_or("")) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "group access denied",
                ));
            }
            change(item);
            item["updated_at"] = json!(utc_now());
            Ok(item.clone())
        })
        .map_err(state_error)
}

fn public_registration(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    result.remove("credential");
    Value::Object(result)
}
fn public_trust(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    result.remove("credential");
    Value::Object(result)
}
fn public_outbound(item: &Value) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    result.remove("credential");
    result.remove("pairing_code");
    Value::Object(result)
}
fn pairing_code() -> String {
    let raw = Uuid::new_v4().simple().to_string().to_ascii_uppercase();
    format!("{}-{}", &raw[..4], &raw[4..8])
}
fn normalize_endpoint(raw: &str) -> Result<String, ApiError> {
    let url = reqwest::Url::parse(raw)
        .map_err(|_| ApiError::bad("issuer_endpoint must be an http(s) URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ApiError::bad("invalid issuer_endpoint"));
    }
    Ok(format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or(""),
        url.port().map_or(String::new(), |port| format!(":{port}"))
    ))
}

async fn post_remote(endpoint: &str, path: &str, body: &Value) -> (Value, String) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(client) => client,
        Err(error) => return (json!({}), error.to_string()),
    };
    match client
        .post(format!("{endpoint}{path}"))
        .json(body)
        .send()
        .await
    {
        Ok(response) => parse_remote(response).await,
        Err(error) => (json!({}), error.to_string()),
    }
}
async fn get_remote(endpoint: &str, path: &str) -> (Value, String) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(client) => client,
        Err(error) => return (json!({}), error.to_string()),
    };
    match client.get(format!("{endpoint}{path}")).send().await {
        Ok(response) => parse_remote(response).await,
        Err(error) => (json!({}), error.to_string()),
    }
}
async fn parse_remote(response: reqwest::Response) -> (Value, String) {
    let status = response.status();
    match response.json::<Value>().await {
        Ok(value) if status.is_success() => {
            (value.get("result").cloned().unwrap_or(value), String::new())
        }
        Ok(value) => (json!({}), value.to_string()),
        Err(error) => (json!({}), error.to_string()),
    }
}
fn state_error(error: io::Error) -> ApiError {
    match error.kind() {
        io::ErrorKind::NotFound => ApiError::not_found(error.to_string()),
        io::ErrorKind::PermissionDenied => ApiError::forbidden(error.to_string()),
        _ => ApiError::bad(error.to_string()),
    }
}
fn io_error(error: io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
