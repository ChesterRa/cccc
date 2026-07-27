use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_contracts::Event;
use cccc_core::{GroupStore, HomeLayout, integration_state, ledger};
use reqwest::StatusCode;
use serde_json::{Map, Value, json};
use uuid::Uuid;

const STORE_KEY: &str = "group_bridge";

pub(crate) async fn try_send(
    home: &HomeLayout,
    client: &DaemonClient,
    args: Map<String, Value>,
) -> Option<Result<Value, String>> {
    let source_group_id = text(&args, "group_id")?.to_owned();
    if let Err(error) = cccc_core::group_bridge_legacy::import_if_changed(home) {
        return Some(Err(error.to_string()));
    }
    let state = match integration_state::global_get(home, STORE_KEY) {
        Ok(state) => state,
        Err(error) => return Some(Err(error.to_string())),
    };
    if let Some(destination_group_id) = text(&args, "dst_group_id") {
        let trust = find_trust(&state, &source_group_id, destination_group_id, None)?;
        return Some(send_new(home, client, args, trust).await);
    }
    let reply_to = text(&args, "reply_to")?;
    let target = match find_event(home, &source_group_id, reply_to) {
        Ok(target) => target,
        Err(error) => return Some(Err(error)),
    };
    if target.data.get("source_platform").and_then(Value::as_str) != Some("group_bridge_session") {
        return None;
    }
    let destination_group_id = target
        .data
        .get("src_group_id")
        .and_then(Value::as_str)
        .or_else(|| target.data.get("source_group_id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let remote_peer_id = target
        .data
        .get("source_user_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let Some(trust) = find_trust(
        &state,
        &source_group_id,
        destination_group_id,
        Some(remote_peer_id),
    ) else {
        return Some(Err(format!(
            "group_bridge_reply_route_not_found: no active Group Bridge route found for reply source group={destination_group_id}"
        )));
    };
    Some(send_reply(home, client, args, &target, trust).await)
}

async fn send_new(
    home: &HomeLayout,
    client: &DaemonClient,
    mut args: Map<String, Value>,
    trust: &Value,
) -> Result<Value, String> {
    let access = trust["remote_access_level"].as_str().unwrap_or("messages");
    if !matches!(access, "messages" | "read" | "full") {
        return Err(format!(
            "remote Group Bridge access={access} does not allow messages"
        ));
    }
    let source_group_id = required_text(&args, "group_id")?.to_owned();
    let destination_group_id = required_text(&args, "dst_group_id")?.to_owned();
    normalize_author_and_recipients(&mut args);
    validate_remote_payload(&args)?;
    validate_peer_insight(&mut args)?;
    let idempotency_key = text(&args, "idempotency_key")
        .or_else(|| text(&args, "client_id"))
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let receipt = deliver(home, &args, trust, &idempotency_key, &idempotency_key, "").await?;

    args.insert("group_id".into(), json!(source_group_id));
    args.insert("dst_group_id".into(), json!(destination_group_id));
    args.insert("client_id".into(), json!(idempotency_key));
    args.insert("delivery_receipt".into(), receipt.clone());
    args.remove("actor_id");
    args.remove("idempotency_key");
    args.remove("reply_to");
    strip_attachment_content(&mut args);
    let local = daemon(client, "send_cross_group_remote_record", args).await?;
    Ok(crate::router::tool_result(json!({
        "source_event":local.get("source_event"),
        "receipt":receipt,
        "transport":"group_bridge_session"
    })))
}

async fn send_reply(
    home: &HomeLayout,
    client: &DaemonClient,
    mut args: Map<String, Value>,
    target: &Event,
    trust: &Value,
) -> Result<Value, String> {
    ensure_message_access(trust)?;
    normalize_author_and_recipients(&mut args);
    let explicit_remote_to = recipients(&args);
    let remote_to = if explicit_remote_to.is_empty() {
        default_remote_reply_recipients(target)
    } else {
        explicit_remote_to
    };
    args.insert("to".into(), json!(remote_to.clone()));
    validate_remote_payload(&args)?;
    validate_peer_insight(&mut args)?;

    let idempotency_key = text(&args, "idempotency_key")
        .or_else(|| text(&args, "client_id"))
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let mut local_args = args.clone();
    local_args.insert("to".into(), json!(["user"]));
    local_args.insert("client_id".into(), json!(idempotency_key));
    local_args.remove("idempotency_key");
    let local = daemon(client, "reply", local_args).await?;
    let event = local
        .get("event")
        .cloned()
        .ok_or("reply response has no event")?;
    let source_event_id = event["id"].as_str().unwrap_or(&idempotency_key);
    let remote_reply_to = target
        .data
        .get("src_event_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let remote_result = deliver(
        home,
        &args,
        trust,
        &format!(
            "reply:{source_event_id}:{}",
            trust["registration_id"].as_str().unwrap_or("remote")
        ),
        source_event_id,
        remote_reply_to,
    )
    .await;
    let group_bridge_reply = match remote_result {
        Ok(receipt) => json!({"receipt":receipt}),
        Err(error) => json!({"error":{"code":"group_bridge_reply_failed","message":error}}),
    };
    Ok(crate::router::tool_result(json!({
        "event":event,
        "group_bridge_reply":group_bridge_reply
    })))
}

async fn deliver(
    home: &HomeLayout,
    args: &Map<String, Value>,
    trust: &Value,
    idempotency_key: &str,
    source_event_id: &str,
    reply_to_remote_event_id: &str,
) -> Result<Value, String> {
    ensure_message_access(trust)?;
    let source_group_id = required_text(args, "group_id")?;
    let endpoint = trust["remote_endpoint"]
        .as_str()
        .map(str::trim)
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .ok_or("remote Group Bridge endpoint is unavailable")?
        .trim_end_matches('/');
    let credential = trust["credential"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("remote Group Bridge credential is unavailable")?;
    let source_title = GroupStore::new(home.clone())
        .and_then(|store| store.load(source_group_id))
        .map(|group| group.title)
        .unwrap_or_default();
    let source_by = text(args, "by")
        .or_else(|| text(args, "actor_id"))
        .unwrap_or("user");
    let mut payload = args.clone();
    for key in [
        "group_id",
        "dst_group_id",
        "actor_id",
        "by",
        "reply_to",
        "idempotency_key",
        "client_id",
        "require_peer_insight",
        "insight",
    ] {
        payload.remove(key);
    }
    if args.get("insight").is_some() {
        let remote_text = cccc_core::peer_insight::append_to_delivery(
            text(args, "text").unwrap_or(""),
            args.get("insight"),
        );
        payload.insert("text".into(), Value::String(remote_text));
    }
    payload.insert("source_group_id".into(), json!(source_group_id));
    payload.insert("src_group_id".into(), json!(source_group_id));
    payload.insert("source_group_title".into(), json!(source_title));
    payload.insert("source_by".into(), json!(source_by));
    payload.insert("src_event_id".into(), json!(source_event_id));
    payload.insert("idempotency_key".into(), json!(idempotency_key));
    if !reply_to_remote_event_id.is_empty() {
        payload.insert("reply_to".into(), json!(reply_to_remote_event_id));
    }

    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let response = http
        .post(format!("{endpoint}/api/group-bridge/session/send"))
        .bearer_auth(credential)
        .json(&Value::Object(payload.clone()))
        .send()
        .await
        .map_err(|error| format!("remote Group Bridge delivery failed: {error}"))?;
    let status = response.status();
    let remote = response
        .json::<Value>()
        .await
        .map_err(|error| format!("remote Group Bridge returned invalid JSON: {error}"))?;
    if status.is_success() {
        return Ok(receipt(&remote, idempotency_key, "group_bridge_session"));
    }
    if matches!(
        status,
        StatusCode::UNAUTHORIZED
            | StatusCode::FORBIDDEN
            | StatusCode::NOT_FOUND
            | StatusCode::METHOD_NOT_ALLOWED
            | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return deliver_via_remote_mcp(
            &http,
            endpoint,
            credential,
            payload,
            idempotency_key,
        )
        .await
        .map_err(|error| {
            format!(
                "remote Group Bridge rejected session delivery with HTTP {status}: {remote}; MCP fallback failed: {error}"
            )
        });
    }
    Err(format!(
        "remote Group Bridge rejected delivery with HTTP {status}: {remote}"
    ))
}

async fn deliver_via_remote_mcp(
    http: &reqwest::Client,
    endpoint: &str,
    credential: &str,
    mut payload: Map<String, Value>,
    idempotency_key: &str,
) -> Result<Value, String> {
    for key in [
        "source_group_id",
        "src_group_id",
        "source_group_title",
        "source_by",
        "src_event_id",
        "idempotency_key",
        "reply_to",
    ] {
        payload.remove(key);
    }
    payload.insert("client_id".into(), json!(idempotency_key));
    let response = http
        .post(format!("{endpoint}/mcp/group-bridge"))
        .bearer_auth(credential)
        .json(&json!({
            "jsonrpc":"2.0",
            "id":idempotency_key,
            "method":"tools/call",
            "params":{"name":"cccc_message_send","arguments":payload}
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
    let remote_event_id = value["result"]["content"]
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
    Ok(json!({
        "status":"delivered",
        "idempotency_key":idempotency_key,
        "remote_event_id":remote_event_id,
        "transport":"group_bridge_mcp"
    }))
}

fn receipt(remote: &Value, idempotency_key: &str, transport: &str) -> Value {
    remote
        .pointer("/result/receipt")
        .or_else(|| remote.get("receipt"))
        .cloned()
        .unwrap_or_else(|| {
            let remote_event_id = remote
                .pointer("/result/event_id")
                .or_else(|| remote.get("event_id"))
                .cloned()
                .unwrap_or(Value::Null);
            json!({
                "status":"delivered",
                "idempotency_key":idempotency_key,
                "remote_event_id":remote_event_id,
                "transport":transport
            })
        })
}

fn ensure_message_access(trust: &Value) -> Result<(), String> {
    let access = trust["remote_access_level"].as_str().unwrap_or("messages");
    matches!(access, "messages" | "read" | "full")
        .then_some(())
        .ok_or_else(|| format!("remote Group Bridge access={access} does not allow messages"))
}

fn find_trust<'a>(
    state: &'a Value,
    source_group_id: &str,
    destination_group_id: &str,
    remote_peer_id: Option<&str>,
) -> Option<&'a Value> {
    trusts(state).iter().find(|trust| {
        trust["group_id"] == source_group_id
            && trust["remote_group_id"] == destination_group_id
            && trust["status"] == "active"
            && remote_peer_id
                .is_none_or(|peer_id| trust["remote_peer_id"].as_str() == Some(peer_id))
    })
}

fn find_event(home: &HomeLayout, group_id: &str, event_id: &str) -> Result<Event, String> {
    let path = GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(group_id))
        .map_err(|error| error.to_string())?;
    ledger::find_event(&path, event_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("event not found: {event_id}"))
}

fn default_remote_reply_recipients(target: &Event) -> Vec<String> {
    let stored = recipients_from_value(target.data.get("remote_reply_to"));
    if !stored.is_empty() {
        return stored;
    }
    if let Some(source_by) = target
        .data
        .get("src_by")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if matches!(source_by, "user" | "@user") {
            return vec!["user".into()];
        }
        if !source_by.starts_with(['@', '#']) && !source_by.starts_with("group_bridge:") {
            return vec![source_by.into()];
        }
    }
    let original = recipients_from_value(target.data.get("to"));
    if !original.is_empty()
        && original.iter().all(|item| {
            matches!(
                item.as_str(),
                "@all" | "@peers" | "@foreman" | "user" | "@user"
            )
        })
    {
        return original;
    }
    Vec::new()
}

fn recipients(args: &Map<String, Value>) -> Vec<String> {
    recipients_from_value(args.get("to"))
}

fn recipients_from_value(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn strip_attachment_content(args: &mut Map<String, Value>) {
    if let Some(attachments) = args.get_mut("attachments").and_then(Value::as_array_mut) {
        for attachment in attachments {
            if let Some(item) = attachment.as_object_mut() {
                item.remove("content_base64");
            }
        }
    }
}

fn normalize_author_and_recipients(args: &mut Map<String, Value>) {
    crate::mapping::normalize_message_author(args);
    if let Some(Value::String(recipient)) = args.get("to").cloned() {
        args.insert("to".into(), json!([recipient]));
    }
}

fn validate_peer_insight(args: &mut Map<String, Value>) -> Result<(), String> {
    let recipients = args.get("to").and_then(Value::as_array).ok_or(
        "remote messages require explicit `to`; use \"@foreman\", \"@all\", or a target actor",
    )?;
    if !recipients
        .iter()
        .filter_map(Value::as_str)
        .any(|recipient| !recipient.trim().is_empty())
    {
        return Err(
            "remote messages require explicit `to`; use \"@foreman\", \"@all\", or a target actor"
                .into(),
        );
    }
    let peer_facing = recipients
        .iter()
        .filter_map(Value::as_str)
        .any(|recipient| !matches!(recipient.trim(), "" | "user" | "@user"));
    let insight = cccc_core::peer_insight::normalize(args.get("insight"))
        .map_err(|error| format!("invalid insight: {error}"))?;
    match insight {
        Some(insight) => {
            args.insert("insight".into(), Value::String(insight));
        }
        None => {
            args.remove("insight");
            if peer_facing {
                return Err(format!(
                    "peer_insight_required: Not sent: this peer-facing message is missing `insight`. {}",
                    *cccc_core::peer_insight::PEER_INSIGHT_REQUIRED_ACTION
                ));
            }
        }
    }
    Ok(())
}

fn validate_remote_payload(args: &Map<String, Value>) -> Result<(), String> {
    if text(args, "suggested_user_message").is_some() {
        return Err(
            "suggested_user_message is only supported for messages in the current group".into(),
        );
    }
    if let Some(priority) = text(args, "priority")
        && !matches!(priority, "normal" | "attention")
    {
        return Err("priority must be normal or attention".into());
    }
    if recipients(args)
        .iter()
        .any(|recipient| recipient.starts_with('#'))
    {
        return Err(
            "cross-group recipients must use `to` for remote actors; `#group` is routing syntax, not a recipient"
                .into(),
        );
    }
    if args
        .get("refs")
        .and_then(Value::as_array)
        .is_some_and(|references| !references.is_empty())
    {
        return Err("refs are not supported by Group Bridge sessions".into());
    }
    Ok(())
}

async fn daemon(
    client: &DaemonClient,
    op: &str,
    args: Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .map_err(|error| error.to_string())?;
    if response.ok {
        Ok(response.result)
    } else {
        Err(response.error.map_or_else(
            || "daemon operation failed".into(),
            |error| format!("{}: {}", error.code, error.message),
        ))
    }
}

fn trusts(state: &Value) -> &[Value] {
    state["trusts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn required_text<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    text(args, key).ok_or_else(|| format!("{key} is required"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use axum::routing::post;
    use axum::{Router, extract::State};
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn local_reply_without_group_bridge_metadata_falls_through() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("local", "").expect("group");
        let mut local = Event::new("chat.message", &group.group_id);
        local.by = "user".into();
        local.data = json!({"text":"question","to":["helper"]})
            .as_object()
            .cloned()
            .expect("local data");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&ledger_path, &local).expect("append local");
        let args = json!({
            "group_id":group.group_id,
            "actor_id":"helper",
            "reply_to":local.id,
            "text":"answer"
        })
        .as_object()
        .cloned()
        .expect("reply args");
        let client = DaemonClient::new(home.clone());

        assert!(try_send(&home, &client, args).await.is_none());
    }

    #[tokio::test]
    async fn remote_reply_without_source_user_id_falls_through() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("receiver", "").expect("group");
        let mut inbound = Event::new("chat.message", &group.group_id);
        inbound.by = "group_bridge:unknown".into();
        inbound.data = json!({
            "text":"question",
            "to":["helper"],
            "source_platform":"group_bridge_session",
            "src_group_id":"g_remote"
        })
        .as_object()
        .cloned()
        .expect("inbound data");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&ledger_path, &inbound).expect("append inbound");
        let args = json!({
            "group_id":group.group_id,
            "actor_id":"helper",
            "reply_to":inbound.id,
            "text":"answer"
        })
        .as_object()
        .cloned()
        .expect("reply args");
        let client = DaemonClient::new(home.clone());

        assert!(try_send(&home, &client, args).await.is_none());
    }

    #[tokio::test]
    async fn remote_reply_is_relayed_with_python_compatible_provenance() {
        let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
        let remote =
            Router::new()
                .route(
                    "/api/group-bridge/session/send",
                    post(
                        |State(captured): State<Arc<Mutex<Vec<Value>>>>,
                         Json(body): Json<Value>| async move {
                            captured.lock().expect("capture").push(body);
                            Json(json!({"ok":true,"result":{"receipt":{
                                "status":"delivered","remote_event_id":"remote-reply",
                                "transport":"group_bridge_session"
                            }}}))
                        },
                    ),
                )
                .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let remote_task = tokio::spawn(async move { axum::serve(listener, remote).await });

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("receiver", "").expect("group");
        cccc_core::group_bridge_legacy::import_if_changed(&home).expect("legacy import");
        integration_state::global_update(&home, STORE_KEY, |state| {
            *state = json!({"trusts":[{
                "registration_id":"registration-1",
                "group_id":group.group_id,
                "remote_group_id":"g_remote",
                "remote_peer_id":"peer-remote",
                "remote_endpoint":endpoint,
                "credential":"secret",
                "remote_access_level":"messages",
                "status":"active"
            }]});
            Ok(())
        })
        .expect("bridge state");
        let mut inbound = Event::new("chat.message", &group.group_id);
        inbound.by = "group_bridge:peer-remote".into();
        inbound.data = json!({
            "text":"question",
            "to":["helper"],
            "source_platform":"group_bridge_session",
            "source_user_id":"peer-remote",
            "src_group_id":"g_remote",
            "src_event_id":"remote-origin-event",
            "src_by":"original-agent",
            "remote_reply_to":["original-agent"]
        })
        .as_object()
        .cloned()
        .expect("inbound data");
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&ledger_path, &inbound).expect("append inbound");

        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&home).await;
        let client = DaemonClient::new(home.clone());
        let args = json!({
            "group_id":group.group_id,
            "actor_id":"helper",
            "reply_to":inbound.id,
            "text":"answer",
            "insight":"The reply should preserve the remote conversation rather than start a disconnected message."
        })
        .as_object()
        .cloned()
        .expect("reply args");

        try_send(&home, &client, args)
            .await
            .expect("remote reply route")
            .expect("relay reply");
        let payload = captured
            .lock()
            .expect("capture")
            .first()
            .cloned()
            .expect("remote payload");
        assert_eq!(payload["source_by"], "helper");
        assert_eq!(payload["reply_to"], "remote-origin-event");
        assert_eq!(payload["to"], json!(["original-agent"]));
        assert!(payload["text"].as_str().is_some_and(|text| {
            text.contains(cccc_core::peer_insight::PEER_PERSPECTIVE_AGENT_LABEL)
        }));
        assert!(
            payload["src_event_id"]
                .as_str()
                .is_some_and(|value| !value.is_empty() && value != "remote-origin-event")
        );
        let events = ledger::read_all(&ledger_path).expect("events");
        let local_reply = events.last().expect("local reply");
        assert_eq!(local_reply.data["reply_to"], inbound.id);
        assert_eq!(local_reply.data["to"], json!(["user"]));

        daemon_task.abort();
        remote_task.abort();
    }

    #[tokio::test]
    async fn trusted_read_route_is_selected_for_remote_message_delivery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let group = GroupStore::new(home.clone())
            .and_then(|store| store.create("source", ""))
            .expect("source group");
        cccc_core::group_bridge_legacy::import_if_changed(&home).expect("legacy import");
        integration_state::global_update(&home, STORE_KEY, |state| {
            *state = json!({
                "trusts":[{
                    "group_id":group.group_id,
                    "remote_group_id":"g_remote",
                    "remote_endpoint":"http://127.0.0.1:9",
                    "credential":"secret",
                    "remote_access_level":"read",
                    "status":"active"
                }]
            });
            Ok(())
        })
        .expect("bridge state");
        let client = DaemonClient::new(home.clone());
        let args = json!({
            "group_id":group.group_id,
            "actor_id":"helper",
            "dst_group_id":"g_remote",
            "to":["@foreman"],
            "text":"需要哪些数据？",
            "insight":"先明确数据契约能降低双方后续集成返工。"
        })
        .as_object()
        .cloned()
        .expect("args");

        let error = try_send(&home, &client, args)
            .await
            .expect("trusted remote route")
            .expect_err("closed test endpoint");
        assert!(error.contains("remote Group Bridge delivery failed"));
    }

    #[tokio::test]
    async fn remote_message_falls_back_to_mcp_for_legacy_peer() {
        let remote = Router::new()
            .route(
                "/api/group-bridge/session/send",
                post(|| async { (StatusCode::NOT_FOUND, Json(json!({"error":"legacy peer"}))) }),
            )
            .route(
                "/mcp/group-bridge",
                post(|Json(request): Json<Value>| async move {
                    assert_eq!(request["params"]["name"], "cccc_message_send");
                    assert_eq!(request["params"]["arguments"]["to"], json!(["@foreman"]));
                    Json(
                        json!({"jsonrpc":"2.0","id":request["id"],"result":{"content":[{
                            "type":"text",
                            "text":"{\"event\":{\"id\":\"legacy-event\"}}"
                        }]}}),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let remote_task = tokio::spawn(async move { axum::serve(listener, remote).await });
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let group = GroupStore::new(home.clone())
            .and_then(|store| store.create("source", ""))
            .expect("source group");
        let args = json!({
            "group_id":group.group_id,
            "by":"helper",
            "to":["@foreman"],
            "text":"legacy",
            "insight":"Compatibility matters because the remote CCCC may be upgraded independently."
        })
        .as_object()
        .cloned()
        .expect("args");
        let trust = json!({
            "registration_id":"registration-legacy",
            "remote_endpoint":endpoint,
            "credential":"secret",
            "remote_access_level":"messages"
        });

        let receipt = deliver(&home, &args, &trust, "retry-key", "source-event", "")
            .await
            .expect("fallback receipt");
        assert_eq!(receipt["transport"], "group_bridge_mcp");
        assert_eq!(receipt["remote_event_id"], "legacy-event");
        remote_task.abort();
    }

    #[tokio::test]
    async fn remote_reply_without_active_route_returns_a_specific_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("receiver", "").expect("group");
        let mut inbound = Event::new("chat.message", &group.group_id);
        inbound.data = json!({
            "source_platform":"group_bridge_session",
            "source_user_id":"peer-missing",
            "src_group_id":"g_missing"
        })
        .as_object()
        .cloned()
        .expect("inbound data");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger"),
            &inbound,
        )
        .expect("append");
        let args = json!({
            "group_id":group.group_id,
            "reply_to":inbound.id,
            "text":"reply"
        })
        .as_object()
        .cloned()
        .expect("args");

        let error = try_send(&home, &DaemonClient::new(home.clone()), args)
            .await
            .expect("remote reply classification")
            .expect_err("missing route");
        assert!(error.contains("group_bridge_reply_route_not_found"));
    }

    #[tokio::test]
    async fn remote_reply_does_not_match_trust_without_remote_peer_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("receiver", "").expect("group");
        integration_state::global_update(&home, STORE_KEY, |state| {
            *state = json!({"trusts":[{
                "group_id":group.group_id,
                "remote_group_id":"g_remote",
                "remote_endpoint":"http://127.0.0.1:9",
                "credential":"secret",
                "remote_access_level":"messages",
                "status":"active"
            }]});
            Ok(())
        })
        .expect("bridge state");
        let mut inbound = Event::new("chat.message", &group.group_id);
        inbound.data = json!({
            "source_platform":"group_bridge_session",
            "source_user_id":"peer-remote",
            "src_group_id":"g_remote"
        })
        .as_object()
        .cloned()
        .expect("inbound data");
        ledger::append(
            &store.ledger_path(&group.group_id).expect("ledger"),
            &inbound,
        )
        .expect("append");
        let args = json!({
            "group_id":group.group_id,
            "reply_to":inbound.id,
            "text":"reply"
        })
        .as_object()
        .cloned()
        .expect("args");

        let error = try_send(&home, &DaemonClient::new(home.clone()), args)
            .await
            .expect("remote reply classification")
            .expect_err("trust without peer must not authorize reply");
        assert!(error.contains("group_bridge_reply_route_not_found"));
    }

    #[test]
    fn remote_peer_messages_require_explicit_recipient_and_insight() {
        let mut missing_to = json!({"to":[],"insight":"higher-level view"})
            .as_object()
            .cloned()
            .expect("args");
        assert!(
            validate_peer_insight(&mut missing_to)
                .expect_err("missing recipient")
                .contains("explicit `to`")
        );

        let mut missing_insight = json!({"to":["@foreman"]})
            .as_object()
            .cloned()
            .expect("args");
        assert!(
            validate_peer_insight(&mut missing_insight)
                .expect_err("missing insight")
                .contains("peer_insight_required")
        );
    }

    #[test]
    fn remote_user_message_does_not_require_peer_insight() {
        let mut args = json!({"to":["user"]}).as_object().cloned().expect("args");
        validate_peer_insight(&mut args).expect("user-facing message");
    }

    #[test]
    fn remote_payload_rejects_local_only_and_unsupported_fields() {
        for args in [
            json!({"to":["@foreman"],"suggested_user_message":"next"}),
            json!({"to":["#remote"]}),
            json!({"to":["@foreman"],"refs":[{"kind":"task_ref"}]}),
            json!({"to":["@foreman"],"priority":"urgent"}),
        ] {
            validate_remote_payload(args.as_object().expect("args"))
                .expect_err("invalid remote payload");
        }
    }

    async fn wait_for_daemon(home: &HomeLayout) {
        let client = DaemonClient::new(home.clone());
        for _ in 0..100 {
            if client
                .call(&DaemonRequest {
                    v: 1,
                    op: "group_list".into(),
                    args: Map::new(),
                })
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("daemon did not start");
    }
}
