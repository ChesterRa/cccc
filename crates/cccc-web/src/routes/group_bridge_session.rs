use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use cccc_contracts::utc_now;
use cccc_core::{GroupStore, ledger};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use uuid::Uuid;

use super::group_bridge_store::{BridgeStore, items, items_mut};
use crate::AppState;
use crate::api::{ApiError, ApiResult, call, success};

#[derive(Debug, Default, Deserialize)]
struct SessionQuery {
    #[serde(default)]
    token: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/group-bridge/session/send", post(receive_http))
        .route("/api/group-bridge/session/ws", get(upgrade))
        .route(
            "/mcp/group-bridge",
            get(mcp_info).post(mcp).options(options),
        )
}

async fn receive_http(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    let registration = authorize(&state, bearer(&headers).unwrap_or(""))?;
    Ok(success(
        receive_delivery(&state, &registration, body).await?,
    ))
}

async fn mcp_info(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let registration = authorize(&state, bearer(&headers).unwrap_or(""))?;
    Ok(Json(json!({
        "name":"cccc-group-bridge-mcp",
        "version":env!("CARGO_PKG_VERSION"),
        "registration_id":registration["registration_id"],
        "group_id":registration["group_id"]
    })))
}

async fn mcp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut request): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let registration = authorize(&state, bearer(&headers).unwrap_or(""))?;
    let access = access_level(&state, &registration)?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if method == "tools/call" {
        let params = request
            .get_mut("params")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| ApiError::bad("tools/call params must be an object"))?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let arguments = params
            .entry("arguments")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| ApiError::bad("tools/call arguments must be an object"))?;
        if !allowed_call(&access, &name, arguments) {
            return Err(ApiError::forbidden(format!(
                "tool is not allowed for group bridge access={access}: {name}"
            )));
        }
        let group_id = registration["group_id"].as_str().unwrap_or("");
        if arguments
            .get("group_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value != group_id)
        {
            return Err(ApiError::forbidden(
                "group bridge cannot access another group",
            ));
        }
        arguments.insert("group_id".into(), json!(group_id));
        arguments.insert(
            "by".into(),
            json!(format!(
                "group_bridge:{}",
                registration["remote_peer_id"].as_str().unwrap_or("remote")
            )),
        );
    }
    let mut response = cccc_mcp::handle_request(&state.home, &request).await;
    if method == "tools/list"
        && let Some(tools) = response
            .get_mut("result")
            .and_then(|value| value.get_mut("tools"))
            .and_then(Value::as_array_mut)
    {
        tools
            .retain(|tool| allowed_call(&access, tool["name"].as_str().unwrap_or(""), &Map::new()));
    }
    Ok(Json(response))
}

async fn options() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let token = if query.token.is_empty() {
        bearer(&headers).unwrap_or("")
    } else {
        &query.token
    };
    let registration = authorize(&state, token)?;
    Ok(ws.on_upgrade(move |socket| session_socket(state, registration, socket)))
}

async fn session_socket(state: AppState, registration: Value, mut socket: WebSocket) {
    let group_id = registration["group_id"].as_str().unwrap_or("").to_owned();
    let _ = socket
        .send(Message::Text(
            json!({"type":"ready","group_id":group_id,"registration_id":registration["registration_id"]})
                .to_string()
                .into(),
        ))
        .await;
    let mut seen = HashSet::new();
    loop {
        tokio::select! {
            incoming = socket.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let response = match serde_json::from_str::<Value>(&text) {
                            Ok(value) if value["type"] == "send" => {
                                match receive_delivery(&state,&registration,value.get("payload").cloned().unwrap_or_else(||json!({}))).await {
                                    Ok(result)=>json!({"type":"receipt","result":result}),
                                    Err(error)=>json!({"type":"error","message":error.to_string()}),
                                }
                            }
                            Ok(value) if value["type"] == "ping" => json!({"type":"pong","ts":utc_now()}),
                            _ => json!({"type":"error","message":"unsupported session message"}),
                        };
                        if socket.send(Message::Text(response.to_string().into())).await.is_err(){break;}
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                let Ok(store)=GroupStore::new(state.home.clone()) else {continue};
                let Ok(path)=store.ledger_path(&group_id) else {continue};
                let Ok(events)=ledger::tail(&path,50) else {continue};
                for event in events {
                    if !seen.insert(event.id.clone()) {continue;}
                    let message=json!({"type":"event","event":event}).to_string();
                    if socket.send(Message::Text(message.into())).await.is_err(){return;}
                }
            }
        }
    }
}

async fn receive_delivery(
    state: &AppState,
    registration: &Value,
    body: Value,
) -> Result<Value, ApiError> {
    let group_id = registration["group_id"].as_str().unwrap_or("");
    let source_group_id = body["source_group_id"].as_str().unwrap_or("");
    if source_group_id != registration["remote_group_id"].as_str().unwrap_or("") {
        return Err(ApiError::forbidden(
            "source group does not match registration",
        ));
    }
    let idempotency_key = body["idempotency_key"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let bridge = BridgeStore::new(&state.home);
    if let Some(receipt) = items(&bridge.load().map_err(io_error)?, "deliveries")
        .iter()
        .find(|item| {
            item["registration_id"] == registration["registration_id"]
                && item["idempotency_key"] == idempotency_key
        })
        .cloned()
    {
        return Ok(json!({"receipt":receipt,"deduped":true}));
    }
    let mut args = body.as_object().cloned().unwrap_or_default();
    args.insert("group_id".into(), json!(group_id));
    args.insert(
        "by".into(),
        json!(format!(
            "group_bridge:{}",
            registration["remote_peer_id"].as_str().unwrap_or("remote")
        )),
    );
    args.insert("source_group_id".into(), json!(source_group_id));
    args.insert(
        "source_group_title".into(),
        body["source_group_title"].clone(),
    );
    args.insert("source_platform".into(), json!("group_bridge_session"));
    args.remove("idempotency_key");
    if let Some(attachments) = args.get_mut("attachments").and_then(Value::as_array_mut) {
        for attachment in attachments {
            let Some(item) = attachment.as_object_mut() else {
                continue;
            };
            let Some(encoded) = item
                .remove("content_base64")
                .and_then(|value| value.as_str().map(str::to_owned))
            else {
                continue;
            };
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| ApiError::bad("invalid remote attachment encoding"))?;
            if bytes.len() > 10 * 1024 * 1024 {
                return Err(ApiError::bad("remote attachment exceeds 10 MiB"));
            }
            let blob = cccc_core::blobs::store(&state.home, group_id, &bytes)
                .map_err(|error| ApiError::bad(error.to_string()))?;
            item.insert("path".into(), json!(blob.path));
            item.insert("bytes".into(), json!(blob.bytes));
            item.insert("sha256".into(), json!(blob.sha256));
        }
    }
    let response = call(state, "send", args).await?;
    let event = response.0["result"]["event"].clone();
    let receipt = json!({
        "registration_id":registration["registration_id"],
        "idempotency_key":idempotency_key,"status":"delivered",
        "event_id":event["id"],"delivered_at":utc_now()
    });
    bridge
        .update(|value| {
            items_mut(value, "deliveries").push(receipt.clone());
            Ok(())
        })
        .map_err(io_error)?;
    Ok(json!({"receipt":receipt,"event":event,"deduped":false}))
}

pub(super) async fn send_remote(
    state: &AppState,
    source_group_id: &str,
    destination_group_id: &str,
    body: &Value,
) -> Option<ApiResult> {
    let bridge = match BridgeStore::new(&state.home).load() {
        Ok(value) => value,
        Err(error) => return Some(Err(io_error(error))),
    };
    let trust = items(&bridge, "trusts").iter().find(|item| {
        item["group_id"] == source_group_id
            && item["remote_group_id"] == destination_group_id
            && item["status"] == "active"
            && item["credential"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
    })?;
    if !matches!(
        trust["remote_access_level"].as_str().unwrap_or("messages"),
        "messages" | "full"
    ) {
        return Some(Err(ApiError::forbidden(
            "remote trust does not allow messages",
        )));
    }
    let endpoint = trust["remote_endpoint"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('/');
    let credential = trust["credential"].as_str().unwrap_or("");
    let idempotency_key = body["client_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let source_title = GroupStore::new(state.home.clone())
        .and_then(|store| store.load(source_group_id))
        .map(|group| group.title)
        .unwrap_or_default();
    let mut payload = body.as_object().cloned().unwrap_or_default();
    payload.remove("dst_group_id");
    payload.insert("source_group_id".into(), json!(source_group_id));
    payload.insert("source_group_title".into(), json!(source_title));
    payload.insert("idempotency_key".into(), json!(idempotency_key));
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(error) => return Some(Err(ApiError::bad(error.to_string()))),
    };
    let response = match client
        .post(format!("{endpoint}/api/group-bridge/session/send"))
        .bearer_auth(credential)
        .json(&Value::Object(payload))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return Some(Err(ApiError::bad(format!(
                "remote delivery failed: {error}"
            ))));
        }
    };
    let status = response.status();
    let remote = match response.json::<Value>().await {
        Ok(value) if status.is_success() => value,
        Ok(value) => {
            return Some(Err(ApiError::bad(format!(
                "remote delivery rejected: {value}"
            ))));
        }
        Err(error) => {
            return Some(Err(ApiError::bad(format!(
                "invalid remote response: {error}"
            ))));
        }
    };
    let mut record = body.as_object().cloned().unwrap_or_default();
    record.insert("group_id".into(), json!(source_group_id));
    record.insert("dst_group_id".into(), json!(destination_group_id));
    record.insert(
        "delivery_receipt".into(),
        remote["result"]["receipt"].clone(),
    );
    let local = match call(state, "send_cross_group_remote_record", record).await {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok(success(json!({
        "source_event":local.0["result"]["source_event"],
        "receipt":remote["result"]["receipt"],
        "transport":"group_bridge_session"
    }))))
}

fn authorize(state: &AppState, credential: &str) -> Result<Value, ApiError> {
    if credential.is_empty() {
        return Err(ApiError::forbidden("group bridge credential required"));
    }
    items(
        &BridgeStore::new(&state.home).load().map_err(io_error)?,
        "registrations",
    )
    .iter()
    .find(|item| item["status"] == "active" && item["credential"].as_str() == Some(credential))
    .cloned()
    .ok_or_else(|| ApiError::forbidden("invalid group bridge credential"))
}

fn access_level(state: &AppState, registration: &Value) -> Result<String, ApiError> {
    Ok(items(
        &BridgeStore::new(&state.home).load().map_err(io_error)?,
        "trusts",
    )
    .iter()
    .find(|item| {
        item["registration_id"] == registration["registration_id"] && item["status"] == "active"
    })
    .and_then(|item| item["access_level"].as_str())
    .unwrap_or("messages")
    .to_owned())
}

fn allowed_call(access: &str, name: &str, arguments: &Map<String, Value>) -> bool {
    if access == "full" {
        return true;
    }
    if matches!(
        name,
        "cccc_message_send" | "cccc_tracked_send" | "cccc_message_reply"
    ) {
        return true;
    }
    if access != "read" {
        return false;
    }
    match name {
        "cccc_help" | "cccc_project_info" | "cccc_context_get" | "cccc_repo" => true,
        "cccc_memory" => matches!(
            arguments
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("search"),
            "search" | "get" | "read" | "profile" | "health"
        ),
        _ => false,
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)?
                .to_str()
                .ok()?
                .strip_prefix("bearer ")
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn io_error(error: std::io::Error) -> ApiError {
    ApiError::bad(error.to_string())
}
