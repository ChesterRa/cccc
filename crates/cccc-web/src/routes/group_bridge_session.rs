use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::{GroupStore, actors, ledger};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::group_bridge_command_sessions;
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
    let registration = if token.is_empty() {
        None
    } else {
        Some(authorize(&state, token)?)
    };
    Ok(ws.on_upgrade(move |socket| session_socket(state, registration, socket)))
}

async fn session_socket(
    state: AppState,
    legacy_registration: Option<Value>,
    mut socket: WebSocket,
) {
    let legacy = legacy_registration.is_some();
    let registration = if let Some(registration) = legacy_registration {
        if socket.send(Message::Text(json!({"type":"ready","group_id":registration["group_id"],"registration_id":registration["registration_id"]}).to_string().into())).await.is_err() {
            return;
        }
        registration
    } else {
        let Some(Ok(Message::Text(text))) = socket.next().await else {
            return;
        };
        let hello = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        let Some(registration) = authorize_signed_hello(&state, &hello) else {
            let _ = socket.send(Message::Text(json!({"ok":false,"error":{"code":"unauthorized_peer","message":"remote peer signature is invalid or not trusted for this group"}}).to_string().into())).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        };
        registration
    };
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
    if !legacy
        && socket
            .send(Message::Text(
                json!({"ok":true,"type":"ready"}).to_string().into(),
            ))
            .await
            .is_err()
    {
        if let Some(close) = close_guard.as_mut() {
            close.close().await;
        }
        return;
    }
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
                                match reauthorize_session(&state, &registration, legacy).map(|active| {
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
                                handle_session_request(&state, &registration, legacy, &value).await
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
                let active = match reauthorize_session(&state, &registration, legacy) {
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

async fn handle_session_request(
    state: &AppState,
    registration: &Value,
    legacy: bool,
    frame: &Value,
) -> (Value, bool) {
    let response_to = frame["request_id"].clone();
    let active = match reauthorize_session(state, registration, legacy) {
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
    let result = if frame["op"] != "remote_send" {
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
        for (field, value) in [
            ("source_group_id", &frame["src_group_id"]),
            ("src_group_id", &frame["src_group_id"]),
            ("idempotency_key", &frame["idempotency_key"]),
        ] {
            if !payload.contains_key(field) && !value.is_null() {
                payload.insert(field.into(), value.clone());
            }
        }
        match receive_delivery(state, &active, Value::Object(payload)).await {
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

fn authorize_signed_hello(state: &AppState, hello: &Value) -> Option<Value> {
    let target_group_id = hello["target_group_id"].as_str()?.trim();
    let src_group_id = hello["src_group_id"].as_str()?.trim();
    let remote_peer_id = hello["remote_peer_id"].as_str()?.trim();
    if !verify_session_hello(hello, remote_peer_id) {
        return None;
    }
    let bridge = BridgeStore::new(&state.home).load().ok()?;
    let trust = items(&bridge, "trusts").iter().find(|item| {
        item["status"] == "active"
            && item["group_id"] == target_group_id
            && item["remote_group_id"] == src_group_id
            && item["remote_peer_id"] == remote_peer_id
    })?;
    Some(
        items(&bridge, "registrations")
            .iter()
            .find(|registration| {
                registration["status"] == "active"
                    && registration["registration_id"] == trust["registration_id"]
                    && registration["group_id"] == target_group_id
                    && registration["remote_group_id"] == src_group_id
                    && registration["remote_peer_id"] == remote_peer_id
            })
            .cloned()
            .unwrap_or_else(|| trust.clone()),
    )
}

fn verify_session_hello(hello: &Value, expected_peer_id: &str) -> bool {
    let Some(public_b64) = hello["public_key"].as_str() else {
        return false;
    };
    let Some(signature_b64) = hello["signature"].as_str() else {
        return false;
    };
    let Ok(public_bytes) = base64::engine::general_purpose::STANDARD.decode(public_b64) else {
        return false;
    };
    let Ok(public): Result<[u8; 32], _> = public_bytes.try_into() else {
        return false;
    };
    let Ok(signature_bytes) = base64::engine::general_purpose::STANDARD.decode(signature_b64)
    else {
        return false;
    };
    let Ok(signature_bytes): Result<[u8; 64], _> = signature_bytes.try_into() else {
        return false;
    };
    if peer_id(&public) != expected_peer_id {
        return false;
    }
    let material = json!({
        "protocol":"/cccc/group_bridge/session-ws/1.0.0",
        "remote_peer_id":expected_peer_id,
        "src_group_id":hello["src_group_id"],
        "target_group_id":hello["target_group_id"]
    })
    .to_string();
    VerifyingKey::from_bytes(&public).is_ok_and(|key| {
        key.verify(
            material.as_bytes(),
            &Signature::from_bytes(&signature_bytes),
        )
        .is_ok()
    })
}

fn peer_id(public: &[u8; 32]) -> String {
    let mut protobuf = vec![0x08, 0x01, 0x12, 32];
    protobuf.extend_from_slice(public);
    let mut multihash = vec![0x00, protobuf.len() as u8];
    multihash.extend(protobuf);
    base58(&multihash)
}

fn base58(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let zeroes = bytes.iter().take_while(|byte| **byte == 0).count();
    let mut digits = vec![0u8];
    for byte in bytes {
        let mut carry = *byte as u32;
        for digit in &mut digits {
            let value = (*digit as u32) * 256 + carry;
            *digit = (value % 58) as u8;
            carry = value / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut output = String::new();
    output.extend(std::iter::repeat_n('1', zeroes));
    output.extend(
        digits
            .iter()
            .rev()
            .map(|digit| ALPHABET[*digit as usize] as char),
    );
    output
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
    if !has_remote_recipient(body.get("to")) {
        return Err(ApiError::bad_code(
            "missing_remote_recipient",
            "remote group bridge messages require explicit to",
            json!({}),
        ));
    }
    if body["refs"]
        .as_array()
        .is_some_and(|references| !references.is_empty())
    {
        return Err(ApiError::bad_code(
            "unsupported_refs",
            "refs are not supported by Group Bridge sessions",
            json!({}),
        ));
    }
    if body
        .get("priority")
        .and_then(Value::as_str)
        .is_some_and(|priority| !matches!(priority, "normal" | "attention"))
    {
        return Err(ApiError::bad_code(
            "invalid_payload",
            "priority must be normal or attention",
            json!({}),
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
    let source_by = args
        .get("source_by")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    let src_event_id = args
        .get("src_event_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    args.insert("group_id".into(), json!(group_id));
    args.insert(
        "by".into(),
        json!(format!("group_bridge:{}", remote_peer_id)),
    );
    args.insert("source_group_id".into(), json!(source_group_id));
    args.insert("src_group_id".into(), json!(source_group_id));
    args.insert("src_event_id".into(), json!(src_event_id));
    args.insert("src_by".into(), json!(source_by));
    args.insert(
        "source_group_title".into(),
        body["source_group_title"].clone(),
    );
    args.insert("source_platform".into(), json!("group_bridge_session"));
    args.insert(
        "source_user_name".into(),
        registration["remote_group_title"].clone(),
    );
    args.insert(
        "source_user_id".into(),
        registration["remote_peer_id"].clone(),
    );
    let remote_reply_to = remote_reply_recipients(&source_by);
    if !remote_reply_to.is_empty() {
        args.insert("remote_reply_to".into(), json!(remote_reply_to));
    }
    args.remove("source_by");
    args.remove("idempotency_key");
    resolve_cross_group_foreman(&state.home, group_id, &mut args)?;
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

fn remote_reply_recipients(source_by: &str) -> Vec<String> {
    let sender = source_by.trim();
    if sender == "user" || sender == "@user" {
        return vec!["user".into()];
    }
    if sender.is_empty() || sender.starts_with(['@', '#']) || sender.starts_with("group_bridge:") {
        return Vec::new();
    }
    vec![sender.into()]
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
        "messages" | "read" | "full"
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
    default_remote_recipient(&mut payload);
    payload.insert("source_group_id".into(), json!(source_group_id));
    payload.insert("src_group_id".into(), json!(source_group_id));
    payload.insert("source_group_title".into(), json!(source_title));
    payload.insert(
        "source_by".into(),
        body.get("by").cloned().unwrap_or_else(|| json!("user")),
    );
    payload.insert(
        "src_event_id".into(),
        body.get("src_event_id")
            .cloned()
            .filter(|value| value.as_str().is_some_and(|value| !value.trim().is_empty()))
            .unwrap_or_else(|| json!(idempotency_key)),
    );
    payload.insert("idempotency_key".into(), json!(idempotency_key));
    if let Some(reply_to) = body
        .get("remote_reply_to_event_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        payload.insert("reply_to".into(), json!(reply_to));
    }
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
        .json(&Value::Object(payload.clone()))
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
        Ok(value)
            if matches!(
                status,
                StatusCode::UNAUTHORIZED
                    | StatusCode::FORBIDDEN
                    | StatusCode::NOT_FOUND
                    | StatusCode::METHOD_NOT_ALLOWED
                    | StatusCode::UNPROCESSABLE_ENTITY
            ) =>
        {
            match send_via_remote_mcp(
                &client,
                endpoint,
                credential,
                Value::Object(payload),
                &idempotency_key,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => {
                    return Some(Err(ApiError::bad(format!(
                        "remote delivery rejected: {value}; MCP fallback failed: {error}"
                    ))));
                }
            }
        }
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
    let receipt = remote
        .pointer("/result/receipt")
        .or_else(|| remote.get("receipt"))
        .cloned()
        .unwrap_or_else(|| json!({"status":"delivered","idempotency_key":idempotency_key}));
    let mut record = body.as_object().cloned().unwrap_or_default();
    default_remote_recipient(&mut record);
    record.insert("group_id".into(), json!(source_group_id));
    record.insert("dst_group_id".into(), json!(destination_group_id));
    record.insert("delivery_receipt".into(), receipt.clone());
    let local = match call(state, "send_cross_group_remote_record", record).await {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok(success(json!({
        "source_event":local.0["result"]["source_event"],
        "receipt":receipt,
        "transport":"group_bridge_session"
    }))))
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

fn resolve_cross_group_foreman(
    home: &cccc_core::HomeLayout,
    group_id: &str,
    args: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    let requested = args
        .get("to")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.len() == 1 && items[0].as_str() == Some(actors::CROSS_GROUP_FOREMAN_RECIPIENT)
        });
    if !requested {
        return Ok(());
    }
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(io_error)?;
    let foreman = actors::unique_available_foreman(&group).map_err(|error| match error {
        actors::UniqueForemanError::NotFound => ApiError::bad_code(
            "foreman_not_found",
            "target group has no available foreman",
            json!({}),
        ),
        actors::UniqueForemanError::NotUnique => ApiError::bad_code(
            "foreman_not_unique",
            "target group has more than one available foreman",
            json!({}),
        ),
    })?;
    args.insert("to".into(), json!([foreman.id]));
    Ok(())
}

async fn send_via_remote_mcp(
    client: &reqwest::Client,
    endpoint: &str,
    credential: &str,
    payload: Value,
    idempotency_key: &str,
) -> Result<Value, String> {
    let mut arguments = payload.as_object().cloned().unwrap_or_default();
    for key in [
        "source_group_id",
        "source_group_title",
        "idempotency_key",
        "dst_group_id",
        "group_id",
        "by",
    ] {
        arguments.remove(key);
    }
    arguments.insert("client_id".into(), json!(idempotency_key));
    let response = client
        .post(format!("{endpoint}/mcp/group-bridge"))
        .bearer_auth(credential)
        .json(&json!({
            "jsonrpc":"2.0","id":idempotency_key,"method":"tools/call",
            "params":{"name":"cccc_message_send","arguments":arguments}
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value = response
        .json::<Value>()
        .await
        .map_err(|error| error.to_string())?;
    if !status.is_success() || value.get("error").is_some() || value["result"]["isError"] == true {
        return Err(value.to_string());
    }
    let event_id = value["result"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text"].as_str())
        .find_map(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|result| {
            result
                .pointer("/event/id")
                .or_else(|| result.pointer("/result/event/id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    Ok(json!({"receipt":{
        "status":"delivered","idempotency_key":idempotency_key,
        "remote_event_id":event_id,"transport":"group_bridge_mcp"
    }}))
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

fn reauthorize(state: &AppState, registration: &Value) -> Result<Value, ApiError> {
    let bridge = BridgeStore::new(&state.home).load().map_err(io_error)?;
    items(&bridge, "registrations")
        .iter()
        .find(|item| item["status"] == "active" && same_registration_snapshot(item, registration))
        .filter(|item| valid_registration(&bridge, item))
        .cloned()
        .ok_or_else(|| ApiError::forbidden("group bridge session is no longer authorized"))
}

fn reauthorize_session(
    state: &AppState,
    registration: &Value,
    legacy: bool,
) -> Result<Value, ApiError> {
    if legacy {
        return reauthorize(state, registration);
    }
    let bridge = BridgeStore::new(&state.home).load().map_err(io_error)?;
    items(&bridge, "trusts")
        .iter()
        .find(|trust| {
            trust["status"] == "active"
                && trust["transport"] == "group_bridge_session"
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
