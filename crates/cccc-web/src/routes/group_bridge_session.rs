use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_contracts::{DaemonRequest, GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION, utc_now};
use cccc_core::{GroupStore, ledger};
use chrono::Utc;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::group_bridge_command_sessions;
use super::group_bridge_session_auth::{
    SessionProtocol, authorize_signed_hello, pin_v2, signed_v2_challenge, signed_v2_ready,
};
use super::group_bridge_store::{BridgeStore, items};
use crate::AppState;
use crate::api::{ApiError, ApiResult, call, success};

const MAX_REMOTE_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_REMOTE_SESSION_JSON_BYTES: usize =
    MAX_REMOTE_ATTACHMENT_BYTES.div_ceil(3) * 4 + 1024 * 1024;

#[derive(Debug, Default, Deserialize)]
struct SessionQuery {
    #[serde(default)]
    token: String,
    #[serde(default)]
    message_contract_version: Option<u64>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/group-bridge/session/send",
            post(receive_http).layer(DefaultBodyLimit::max(MAX_REMOTE_SESSION_JSON_BYTES)),
        )
        .route("/api/group-bridge/session/ws", get(upgrade_v1))
        .route("/api/group-bridge/session/ws/v2", get(upgrade_v2))
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
    require_message_contract_version(&body)?;
    let registration = authorize(&state, bearer(&headers).unwrap_or(""))?;
    let result = match body["op"].as_str().unwrap_or("") {
        "reply_request_cancel" => receive_reply_request_cancel(&state, &registration, body).await?,
        "remote_send" => receive_delivery(&state, &registration, body).await?,
        _ => {
            return Err(ApiError::bad_code(
                "unsupported_op",
                "unsupported Group Bridge session operation",
                json!({}),
            ));
        }
    };
    Ok(success(result))
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
    let grant = group_bridge_command_sessions::access_grant(&state, &registration)?;
    let access = grant.level.as_str();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut bridge_tool_name = None;
    let mut terminate_session = false;
    let mut bridge_session_id = None;
    if method == "tools/call" {
        let request_id = request["id"].clone();
        let params = request
            .get_mut("params")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| ApiError::bad("tools/call params must be an object"))?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let local_name = local_bridge_tool(&name);
        params.insert("name".into(), json!(local_name));
        let arguments = params
            .entry("arguments")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| ApiError::bad("tools/call arguments must be an object"))?;
        if !allowed_call(access, &name, arguments) {
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
        if name == "cccc_remote_write_stdin" {
            group_bridge_command_sessions::require(arguments, &registration, &grant)?;
            bridge_session_id = arguments
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            terminate_session = arguments
                .get("terminate")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        bridge_tool_name = Some(name.clone());
        if name == "cccc_remote_git" {
            normalize_remote_git(arguments)?;
        }
        if name == "cccc_remote_access" {
            let payload = bridge_access_payload(&registration, access);
            return Ok(Json(json!({
                "jsonrpc":"2.0","id":request_id,
                "result":bridge_tool_result(payload)
            })));
        }
    }
    let mut response = cccc_mcp::handle_request(&state.home, &request).await;
    if let Some(name) = bridge_tool_name.as_deref() {
        group_bridge_command_sessions::update(
            name,
            &registration,
            &grant,
            &response,
            bridge_session_id.as_deref(),
            terminate_session,
        )?;
    }
    if method == "tools/list"
        && let Some(tools) = response
            .get_mut("result")
            .and_then(|value| value.get_mut("tools"))
            .and_then(Value::as_array_mut)
    {
        tools.retain(|tool| allowed_call(access, tool["name"].as_str().unwrap_or(""), &Map::new()));
    }
    Ok(Json(response))
}

async fn options() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn upgrade_v1(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if !query.token.is_empty() {
        return Err(ApiError::forbidden(
            "Group Bridge WebSocket query tokens are not supported",
        ));
    }
    let token = bearer(&headers).unwrap_or("");
    let registration = if token.is_empty() {
        None
    } else {
        if query.message_contract_version != Some(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION) {
            return Err(contract_version_mismatch());
        }
        Some(authorize_v1_session(&state, token)?)
    };
    Ok(ws
        .on_upgrade(move |socket| session_socket(state, registration, socket, SessionProtocol::V1)))
}

async fn upgrade_v2(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if !query.token.is_empty() {
        return Err(ApiError::forbidden(
            "Group Bridge WebSocket query tokens are not supported",
        ));
    }
    Ok(ws.on_upgrade(move |socket| session_socket(state, None, socket, SessionProtocol::V2)))
}

async fn session_socket(
    state: AppState,
    legacy_registration: Option<Value>,
    mut socket: WebSocket,
    protocol: SessionProtocol,
) {
    let legacy = legacy_registration.is_some();
    let mut v2_transcript = None;
    let registration = if let Some(registration) = legacy_registration {
        if socket.send(Message::Text(json!({"type":"ready","group_id":registration["group_id"],"registration_id":registration["registration_id"],"message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION}).to_string().into())).await.is_err() {
            return;
        }
        registration
    } else {
        let challenge = if protocol == SessionProtocol::V2 {
            let Some(challenge) = signed_v2_challenge(&state) else {
                return;
            };
            if socket
                .send(Message::Text(challenge.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
            Some(challenge)
        } else {
            None
        };
        let Some(Ok(Message::Text(text))) = socket.next().await else {
            return;
        };
        let hello = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        if require_message_contract_version(&hello).is_err() {
            let _ = socket.send(Message::Text(json!({"ok":false,"error":{"code":"contract_version_mismatch","message":"Group Bridge message contract version does not match","details":{"expected":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION}}}).to_string().into())).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
        if challenge.as_ref().is_some_and(challenge_expired) {
            let _ = socket.send(Message::Text(json!({"ok":false,"error":{"code":"challenge_expired","message":"Group Bridge session challenge expired"}}).to_string().into())).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
        let Some(registration) =
            authorize_signed_hello(&state, &hello, protocol, challenge.as_ref())
        else {
            let _ = socket.send(Message::Text(json!({"ok":false,"error":{"code":"unauthorized_peer","message":"remote peer signature is invalid or not trusted for this group"}}).to_string().into())).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        };
        if let Some(challenge) = challenge {
            v2_transcript = Some((hello, challenge));
        }
        registration
    };
    let ready = match v2_transcript.as_ref() {
        Some((hello, challenge)) => {
            let Some(ready) = signed_v2_ready(&state, hello, challenge) else {
                return;
            };
            ready
        }
        None => {
            json!({"ok":true,"type":"ready","message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION})
        }
    };
    if !legacy
        && socket
            .send(Message::Text(ready.to_string().into()))
            .await
            .is_err()
    {
        return;
    }
    if protocol == SessionProtocol::V2 && pin_v2(&state, &registration).is_none() {
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    let route_args = json!({
        "group_id":registration["group_id"],"remote_group_id":registration["remote_group_id"],
        "remote_peer_id":registration["remote_peer_id"]
    });
    let generation = daemon_value(&state, "group_bridge_session_open", &route_args)
        .await
        .and_then(|opened| opened["generation"].as_str().map(str::to_owned));
    let mut close_guard = generation.as_deref().map(|generation| {
        super::group_bridge_close::SessionClose::new(state.clone(), &route_args, generation)
    });
    let mut seen = super::group_bridge_seen::SeenEvents::default();
    let mut session_poll = tokio::time::interval(std::time::Duration::from_millis(25));
    session_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut shutdown = state.shutdown.subscribe();
    'session: loop {
        tokio::select! {
            _ = shutdown.recv() => {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            incoming = socket.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let (response, close) = match serde_json::from_str::<Value>(&text) {
                            Ok(value) if value["type"] == "send" => {
                                match reauthorize_session(&state, &registration, legacy, protocol).map(|active| {
                                    (active, value.get("payload").cloned().unwrap_or_else(||json!({})))
                                }) {
                                    Ok((active, payload)) => (match receive_delivery(&state,&active,payload).await {
                                        Ok(result)=>json!({"type":"receipt","result":result}),
                                        Err(error)=>json!({"type":"error","message":error.to_string()}),
                                    }, false),
                                    Err(error)=>(json!({"type":"error","message":error.to_string()}), true),
                                }
                            }
                            Ok(value) if value["type"] == "response" => {
                                let Some(generation) = generation.as_deref() else { continue };
                                let mut complete = route_args.clone();
                                complete["generation"] = json!(generation);
                                complete["response_to"] = value["response_to"].clone();
                                complete["result"] = value.get("result").cloned().unwrap_or_else(||json!({"ok":false}));
                                let _ = daemon_value(&state, "group_bridge_session_complete", &complete).await;
                                continue;
                            }
                            Ok(value) if value["type"] == "request" => {
                                handle_session_request(&state, &registration, legacy, protocol, &value).await
                            }
                            Ok(value) if value["type"] == "ping" => (json!({"type":"pong","ts":utc_now()}), false),
                            _ => (json!({"type":"error","message":"unsupported session message"}), false),
                        };
                        if socket.send(Message::Text(response.to_string().into())).await.is_err(){break;}
                        if close {
                            let _ = socket.send(Message::Close(None)).await;
                            break 'session;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = session_poll.tick(), if generation.is_some() => {
                match poll_session_request(&state, &route_args, generation.as_deref()).await {
                    Some(request) if !request.is_null() => {
                        if socket.send(Message::Text(request.to_string().into())).await.is_err() { break; }
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            () = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                let active = match reauthorize_session(&state, &registration, legacy, protocol) {
                    Ok(active) => active,
                    Err(error) => {
                        let _ = socket.send(Message::Text(
                            json!({"type":"error","message":error.to_string()}).to_string().into(),
                        )).await;
                        let _ = socket.send(Message::Close(None)).await;
                        break 'session;
                    }
                };
                let Ok(group_id)=required_session_field(&active,"group_id") else {break 'session};
                let Ok(store)=GroupStore::new(state.home.clone()) else {continue};
                let Ok(path)=store.ledger_path(group_id) else {continue};
                let Ok(events)=ledger::tail(&path,50) else {continue};
                for event in events {
                    if !seen.insert(event.id.clone()) {continue;}
                    let message=json!({"type":"event","event":event}).to_string();
                    if socket.send(Message::Text(message.into())).await.is_err(){break 'session;}
                }
            }
        }
    }
    if let Some(close) = close_guard.as_mut() {
        close.close().await;
    }
}

fn challenge_expired(challenge: &Value) -> bool {
    challenge["expires_at"]
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|expires_at| expires_at.with_timezone(&Utc) <= Utc::now())
}

async fn handle_session_request(
    state: &AppState,
    registration: &Value,
    legacy: bool,
    protocol: SessionProtocol,
    frame: &Value,
) -> (Value, bool) {
    let response_to = frame["request_id"].clone();
    let active = match reauthorize_session(state, registration, legacy, protocol) {
        Ok(active) => active,
        Err(error) => {
            return (
                json!({
                    "type":"response",
                    "response_to":response_to,
                    "result":{"ok":false,"error":{"code":"permission_denied","message":error.to_string()}}
                }),
                true,
            );
        }
    };
    let operation = frame["op"].as_str().unwrap_or("");
    let result = if frame["message_contract_version"].as_u64()
        != Some(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION)
    {
        json!({
            "ok":false,
            "error":{"code":"contract_version_mismatch","message":"Group Bridge message contract version does not match"}
        })
    } else if !matches!(operation, "remote_send" | "reply_request_cancel") {
        json!({
            "ok":false,
            "error":{"code":"unsupported_op","message":"unsupported Group Bridge session operation"}
        })
    } else {
        let mut payload = frame
            .get("payload")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if operation == "remote_send" {
            payload.insert(
                "message_contract_version".into(),
                json!(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION),
            );
            for (field, value) in [
                ("source_group_id", &frame["src_group_id"]),
                ("src_group_id", &frame["src_group_id"]),
                ("idempotency_key", &frame["idempotency_key"]),
            ] {
                if !payload.contains_key(field) && !value.is_null() {
                    payload.insert(field.into(), value.clone());
                }
            }
        }
        let delivery = if operation == "reply_request_cancel" {
            receive_reply_request_cancel(state, &active, Value::Object(payload)).await
        } else {
            receive_delivery(state, &active, Value::Object(payload)).await
        };
        match delivery {
            Ok(result) => result,
            Err(error) => json!({
                "ok":false,
                "error":{"code":"remote_delivery_failed","message":error.to_string()}
            }),
        }
    };
    (
        json!({"type":"response","response_to":response_to,"result":result}),
        false,
    )
}

async fn poll_session_request(
    state: &AppState,
    route: &Value,
    generation: Option<&str>,
) -> Option<Value> {
    let Some(generation) = generation else {
        return std::future::pending().await;
    };
    let mut args = route.clone();
    args["generation"] = json!(generation);
    args["timeout_ms"] = json!(1);
    daemon_value(state, "group_bridge_session_poll", &args)
        .await
        .map(|value| value["request"].clone())
}

async fn daemon_value(state: &AppState, op: &str, args: &Value) -> Option<Value> {
    let response = state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        })
        .await
        .ok()?;
    response.ok.then_some(Value::Object(response.result))
}

fn require_message_contract_version(value: &Value) -> Result<(), ApiError> {
    if value["message_contract_version"].as_u64() == Some(GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION) {
        Ok(())
    } else {
        Err(contract_version_mismatch())
    }
}

fn contract_version_mismatch() -> ApiError {
    ApiError::conflict(
        "contract_version_mismatch",
        "Group Bridge message contract version does not match",
        json!({"expected":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION}),
    )
}

async fn receive_delivery(
    state: &AppState,
    registration: &Value,
    body: Value,
) -> Result<Value, ApiError> {
    let group_id = required_session_field(registration, "group_id")?;
    let remote_group_id = required_session_field(registration, "remote_group_id")?;
    let remote_peer_id = required_session_field(registration, "remote_peer_id")?;
    let source_group_id = body
        .get("source_group_id")
        .and_then(Value::as_str)
        .or_else(|| body.get("src_group_id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::forbidden("source group is required"))?;
    if source_group_id != remote_group_id {
        return Err(ApiError::forbidden(
            "source group does not match registration",
        ));
    }
    let idempotency_key = body["idempotency_key"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let response = call(
        state,
        "group_bridge_receive_remote_send",
        json!({
            "target_group_id":group_id,
            "src_group_id":remote_group_id,
            "remote_peer_id":remote_peer_id,
            "idempotency_key":idempotency_key,
            "payload":body
        })
        .as_object()
        .cloned()
        .expect("Group Bridge receive request is an object"),
    )
    .await?;
    let mut result = response.0["result"].clone();
    if let Some(fields) = result.as_object_mut() {
        fields.remove("ok");
    }
    Ok(result)
}

async fn receive_reply_request_cancel(
    state: &AppState,
    registration: &Value,
    body: Value,
) -> Result<Value, ApiError> {
    let group_id = required_session_field(registration, "group_id")?;
    let remote_group_id = required_session_field(registration, "remote_group_id")?;
    let remote_peer_id = required_session_field(registration, "remote_peer_id")?;
    let source_group_id = body
        .get("source_group_id")
        .and_then(Value::as_str)
        .or_else(|| body.get("src_group_id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::forbidden("source group is required"))?;
    if source_group_id != remote_group_id {
        return Err(ApiError::forbidden(
            "source group does not match registration",
        ));
    }
    let payload = body.get("payload").cloned().unwrap_or_else(|| body.clone());
    let response = call(
        state,
        "group_bridge_receive_reply_request_cancel",
        json!({
            "target_group_id":group_id,
            "src_group_id":remote_group_id,
            "remote_peer_id":remote_peer_id,
            "payload":payload
        })
        .as_object()
        .cloned()
        .expect("Group Bridge cancellation request is an object"),
    )
    .await?;
    let mut result = response.0["result"].clone();
    if let Some(fields) = result.as_object_mut() {
        fields.remove("ok");
    }
    Ok(result)
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
    })?;
    if !matches!(
        trust["remote_access_level"].as_str().unwrap_or("messages"),
        "messages" | "read" | "full"
    ) {
        return Some(Err(ApiError::forbidden(
            "remote trust does not allow messages",
        )));
    }
    let registration_id = ["registration_id", "trust_id"]
        .into_iter()
        .find_map(|field| {
            trust[field]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .map(str::to_owned);
    let Some(registration_id) = registration_id else {
        return Some(Err(ApiError::bad(
            "active Group Bridge trust is missing route identity",
        )));
    };
    let idempotency_key = body["client_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let mut payload = Map::new();
    for field in [
        "text",
        "format",
        "message_mode",
        "to",
        "refs",
        "attachments",
    ] {
        if let Some(value) = body.get(field).cloned() {
            payload.insert(field.into(), value);
        }
    }
    default_remote_recipient(&mut payload);
    let mut args = json!({
        "group_id":source_group_id,
        "registration_id":registration_id,
        "idempotency_key":idempotency_key,
        "by":body.get("by").cloned().unwrap_or_else(|| json!("user")),
        "insight":body.get("insight").cloned().unwrap_or(Value::Null),
        "require_peer_insight":body.get("require_peer_insight").cloned().unwrap_or(Value::Bool(false)),
        "payload":payload
    })
    .as_object()
    .cloned()
    .expect("remote send request is an object");
    if let Some(value) = body.get("src_event_id").cloned() {
        args.insert("source_event_id".into(), value);
    }
    if let Some(value) = body.get("remote_reply_to_event_id").cloned() {
        args.insert("reply_to_remote_event_id".into(), value);
    }
    Some(call(state, "remote_send", args).await)
}

fn has_remote_recipient(value: Option<&Value>) -> bool {
    value.and_then(Value::as_array).is_some_and(|recipients| {
        recipients
            .iter()
            .filter_map(Value::as_str)
            .any(|recipient| !recipient.trim().is_empty())
    })
}

fn default_remote_recipient(args: &mut Map<String, Value>) {
    if !has_remote_recipient(args.get("to")) {
        args.insert("to".into(), json!(["@foreman"]));
    }
}

fn authorize(state: &AppState, credential: &str) -> Result<Value, ApiError> {
    if credential.is_empty() {
        return Err(ApiError::forbidden("group bridge credential required"));
    }
    let bridge = BridgeStore::new(&state.home).load().map_err(io_error)?;
    items(&bridge, "registrations")
        .iter()
        .find(|item| item["status"] == "active" && item["credential"].as_str() == Some(credential))
        .filter(|item| valid_registration(&bridge, item))
        .cloned()
        .ok_or_else(|| ApiError::forbidden("invalid group bridge credential"))
}

fn authorize_v1_session(state: &AppState, credential: &str) -> Result<Value, ApiError> {
    let registration = authorize(state, credential)?;
    reauthorize_session(state, &registration, true, SessionProtocol::V1)
}

fn reauthorize(
    state: &AppState,
    registration: &Value,
    protocol: SessionProtocol,
) -> Result<Value, ApiError> {
    let bridge = BridgeStore::new(&state.home).load().map_err(io_error)?;
    items(&bridge, "registrations")
        .iter()
        .find(|item| item["status"] == "active" && same_registration_snapshot(item, registration))
        .filter(|item| valid_registration_for_protocol(&bridge, item, protocol))
        .cloned()
        .ok_or_else(|| ApiError::forbidden("group bridge session is no longer authorized"))
}

fn reauthorize_session(
    state: &AppState,
    registration: &Value,
    legacy: bool,
    protocol: SessionProtocol,
) -> Result<Value, ApiError> {
    if legacy {
        return reauthorize(state, registration, protocol);
    }
    let bridge = BridgeStore::new(&state.home).load().map_err(io_error)?;
    items(&bridge, "trusts")
        .iter()
        .find(|trust| {
            trust["status"] == "active"
                && trust["transport"] == "group_bridge_session"
                && session_protocol_allowed(trust, protocol)
                && [
                    "registration_id",
                    "group_id",
                    "remote_group_id",
                    "remote_peer_id",
                ]
                .into_iter()
                .all(|field| trust[field] == registration[field])
        })
        .cloned()
        .ok_or_else(|| ApiError::forbidden("group bridge session is no longer authorized"))
}

fn valid_registration_for_protocol(
    bridge: &Value,
    registration: &Value,
    protocol: SessionProtocol,
) -> bool {
    valid_registration(bridge, registration)
        && items(bridge, "trusts").iter().any(|trust| {
            trust["status"] == "active"
                && group_bridge_command_sessions::trust_matches_registration(trust, registration)
                && session_protocol_allowed(trust, protocol)
        })
}

fn session_protocol_allowed(trust: &Value, protocol: SessionProtocol) -> bool {
    let minimum = trust["min_session_protocol"].as_u64().unwrap_or(1);
    match protocol {
        SessionProtocol::V1 => minimum < 2,
        SessionProtocol::V2 => minimum >= 2,
    }
}

fn valid_registration(bridge: &Value, registration: &Value) -> bool {
    registration["transport"].as_str() == Some("group_bridge_session")
        && [
            "registration_id",
            "group_id",
            "remote_group_id",
            "remote_peer_id",
        ]
        .into_iter()
        .all(|field| {
            registration[field]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty())
        })
        && items(bridge, "trusts").iter().any(|trust| {
            trust["status"] == "active"
                && group_bridge_command_sessions::trust_matches_registration(trust, registration)
        })
}

fn same_registration_snapshot(current: &Value, snapshot: &Value) -> bool {
    [
        "registration_id",
        "credential",
        "transport",
        "group_id",
        "remote_group_id",
        "remote_peer_id",
    ]
    .into_iter()
    .all(|field| current[field] == snapshot[field])
}

fn required_session_field<'a>(registration: &'a Value, field: &str) -> Result<&'a str, ApiError> {
    registration
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::forbidden(format!("group bridge registration lacks {field}")))
}

fn allowed_call(access: &str, name: &str, arguments: &Map<String, Value>) -> bool {
    if matches!(
        name,
        "cccc_message_send" | "cccc_tracked_send" | "cccc_message_reply" | "cccc_remote_access"
    ) {
        return true;
    }
    if name == "cccc_remote_git"
        && matches!(
            arguments.get("action").and_then(Value::as_str),
            Some("add" | "commit")
        )
    {
        return access == "full";
    }
    if matches!(access, "read" | "full")
        && matches!(
            name,
            "cccc_remote_context" | "cccc_remote_repo" | "cccc_remote_git"
        )
    {
        return true;
    }
    access == "full"
        && matches!(
            name,
            "cccc_remote_repo_edit"
                | "cccc_remote_apply_patch"
                | "cccc_remote_shell"
                | "cccc_remote_exec_command"
                | "cccc_remote_write_stdin"
        )
}

fn local_bridge_tool(name: &str) -> &str {
    match name {
        "cccc_remote_context" => "cccc_context_get",
        "cccc_remote_repo" => "cccc_repo",
        "cccc_remote_git" => "cccc_git",
        "cccc_remote_repo_edit" => "cccc_repo_edit",
        "cccc_remote_apply_patch" => "cccc_apply_patch",
        "cccc_remote_shell" => "cccc_shell",
        "cccc_remote_exec_command" => "cccc_exec_command",
        "cccc_remote_write_stdin" => "cccc_write_stdin",
        _ => name,
    }
}

fn normalize_remote_git(arguments: &mut Map<String, Value>) -> Result<(), ApiError> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("status");
    if !matches!(action, "status" | "diff" | "log" | "add" | "commit") {
        return Err(ApiError::bad(
            "remote git action must be status, diff, log, add, or commit",
        ));
    }
    Ok(())
}

fn bridge_access_payload(registration: &Value, access: &str) -> Value {
    json!({
        "remote_group_id":registration["group_id"],
        "access_level":access,
        "permissions":{
            "messages":true,
            "read":matches!(access,"read"|"full"),
            "full":access=="full"
        }
    })
}

fn bridge_tool_result(payload: Value) -> Value {
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
    json!({"content":[{"type":"text","text":text}],"structuredContent":payload})
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

#[cfg(test)]
mod tests {
    use super::same_registration_snapshot;
    use serde_json::json;

    #[test]
    fn websocket_snapshot_rejects_in_place_identity_change() {
        let snapshot = json!({
            "registration_id":"greg_test","credential":"secret",
            "transport":"group_bridge_session","group_id":"g_local",
            "remote_group_id":"g_remote","remote_peer_id":"peer_remote"
        });
        for field in [
            "registration_id",
            "credential",
            "transport",
            "group_id",
            "remote_group_id",
            "remote_peer_id",
        ] {
            let mut changed = snapshot.clone();
            changed[field] = json!("changed");
            assert!(!same_registration_snapshot(&changed, &snapshot), "{field}");
        }
        assert!(same_registration_snapshot(&snapshot, &snapshot));
    }
}
