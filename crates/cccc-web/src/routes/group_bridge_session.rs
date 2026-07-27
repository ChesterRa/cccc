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
    let source_group_id = body["source_group_id"]
        .as_str()
        .or_else(|| body["src_group_id"].as_str())
        .unwrap_or("");
    if source_group_id != registration["remote_group_id"].as_str().unwrap_or("") {
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
        json!(format!(
            "group_bridge:{}",
            registration["remote_peer_id"].as_str().unwrap_or("remote")
        )),
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
    items(
        &BridgeStore::new(&state.home).load().map_err(io_error)?,
        "registrations",
    )
    .iter()
    .find(|item| item["status"] == "active" && item["credential"].as_str() == Some(credential))
    .cloned()
    .ok_or_else(|| ApiError::forbidden("invalid group bridge credential"))
}

fn allowed_call(access: &str, name: &str, arguments: &Map<String, Value>) -> bool {
    let _ = arguments;
    if matches!(
        name,
        "cccc_message_send" | "cccc_tracked_send" | "cccc_message_reply" | "cccc_remote_access"
    ) {
        return true;
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
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "status".into());
    let mut args = match action.as_str() {
        "status" => vec![json!("status"), json!("--short")],
        "diff" => vec![json!("diff")],
        "log" => vec![json!("log"), json!("--oneline"), json!("-n"), json!("50")],
        _ => {
            return Err(ApiError::bad(
                "remote git action must be status, diff, or log",
            ));
        }
    };
    if let Some(path) = arguments
        .remove("path")
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.is_empty())
    {
        args.push(json!("--"));
        args.push(json!(path));
    }
    arguments.insert("args".into(), Value::Array(args));
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
