use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use uuid::Uuid;

pub(crate) fn apply_cross_group_default(args: &mut Map<String, Value>) -> Result<(), String> {
    let cross_group = match (text(args, "group_id"), text(args, "dst_group_id")) {
        (Some(source), Some(destination)) => source != destination,
        _ => false,
    };
    if !cross_group {
        return Ok(());
    }
    if args.contains_key("to") {
        let valid = args.get("to").is_some_and(|value| match value {
            Value::String(value) => !value.trim().is_empty(),
            Value::Array(values) => {
                !values.is_empty()
                    && values
                        .iter()
                        .all(|value| value.as_str().is_some_and(|value| !value.trim().is_empty()))
            }
            _ => false,
        });
        if !valid {
            return Err(
                "invalid_recipient: cross-group to must be a non-empty string or string array"
                    .into(),
            );
        }
    } else {
        args.insert(
            "to".into(),
            json!([cccc_core::actors::CROSS_GROUP_FOREMAN_RECIPIENT]),
        );
    }
    Ok(())
}

pub(crate) async fn try_send(
    home: &HomeLayout,
    client: &DaemonClient,
    args: Map<String, Value>,
) -> Option<Result<Value, String>> {
    let source_group_id = text(&args, "group_id")?.to_owned();
    if let Some(destination_group_id) = text(&args, "dst_group_id") {
        let state = match cccc_core::group_bridge_legacy::load(home) {
            Ok(state) => state,
            Err(error) => return Some(Err(error.to_string())),
        };
        let trust = find_trust(&state, &source_group_id, destination_group_id, None)?;
        return Some(send_new(home, client, args, trust).await);
    }
    // Replies, including Group Bridge replies, are owned by the daemon. Keeping
    // one routing authority prevents the MCP adapter and Web path from applying
    // different default recipients or relaying the same reply twice.
    None
}

async fn send_new(
    _home: &HomeLayout,
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
    let registration_id = ["registration_id", "trust_id"]
        .into_iter()
        .find_map(|field| {
            trust[field]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .ok_or("active Group Bridge trust is missing route identity")?
        .to_owned();
    normalize_author_and_recipients(&mut args);
    validate_remote_payload(&args)?;
    validate_peer_insight(&mut args)?;
    let idempotency_key = text(&args, "idempotency_key")
        .or_else(|| text(&args, "client_id"))
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let source_by = text(&args, "by").unwrap_or("user").to_owned();
    let insight = args.get("insight").cloned().unwrap_or(Value::Null);
    let require_peer_insight = args
        .get("require_peer_insight")
        .cloned()
        .unwrap_or(Value::Bool(true));
    let mut payload = Map::new();
    for field in ["text", "format", "to", "refs", "attachments"] {
        if let Some(value) = args.get(field).cloned() {
            payload.insert(field.into(), value);
        }
    }
    payload.insert(
        "message_mode".into(),
        Value::String(remote_message_mode(&args)?.to_owned()),
    );
    let result = daemon(
        client,
        "remote_send",
        json!({
            "group_id":source_group_id,
            "registration_id":registration_id,
            "idempotency_key":idempotency_key,
            "by":source_by,
            "insight":insight,
            "require_peer_insight":require_peer_insight,
            "payload":payload
        })
        .as_object()
        .cloned()
        .expect("remote send request is an object"),
    )
    .await?;
    Ok(crate::router::tool_result(Value::Object(result)))
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

fn normalize_author_and_recipients(args: &mut Map<String, Value>) {
    crate::argument_normalization::normalize_message_author(args);
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
    if ["priority", "reply_required", "requires_ack"]
        .iter()
        .any(|field| args.contains_key(*field))
    {
        return Err(
            "use mode; legacy priority/reply_required/requires_ack fields are not supported".into(),
        );
    }
    let mode = remote_message_mode(args)?;
    if mode == "request_reply"
        && (recipients(args).is_empty()
            || recipients(args)
                .iter()
                .any(|recipient| matches!(recipient.as_str(), "@all" | "@peers" | "@foreman")))
    {
        return Err("request_reply requires one or more explicit concrete recipients".into());
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

fn remote_message_mode(args: &Map<String, Value>) -> Result<&str, String> {
    let public_mode = text(args, "mode");
    let canonical_mode = text(args, "message_mode");
    if public_mode.is_some() && canonical_mode.is_some() && public_mode != canonical_mode {
        return Err("mode and message_mode must not disagree".into());
    }
    let mode = canonical_mode.or(public_mode).unwrap_or("mail");
    if !matches!(mode, "send" | "request_reply" | "mail") {
        return Err("mode must be mail, send, or request_reply".into());
    }
    Ok(mode)
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
    use cccc_contracts::{Actor, Event};
    use cccc_core::{GroupStore, ledger};
    use std::sync::{Arc, Mutex};

    fn seed_bridge(home: &HomeLayout, value: Value) {
        cccc_core::group_bridge_legacy::update(home, |state| {
            state.clear();
            state.extend(value.as_object().cloned().unwrap_or_default());
            Ok(())
        })
        .expect("bridge state");
    }

    fn source_group_with_helper(home: &HomeLayout) -> cccc_core::GroupDoc {
        let store = GroupStore::new(home.clone()).expect("group store");
        let group = store.create("source", "").expect("source group");
        store
            .mutate(&group.group_id, |current| {
                current.actors.push(Actor::new("helper"));
                Ok(current.clone())
            })
            .expect("helper actor")
    }

    #[test]
    fn cross_group_default_intent_only_applies_when_recipient_is_omitted() {
        let mut omitted = json!({"group_id":"g_local","dst_group_id":"g_remote"})
            .as_object()
            .cloned()
            .expect("omitted args");
        apply_cross_group_default(&mut omitted).expect("default");
        assert_eq!(
            omitted["to"],
            json!([cccc_core::actors::CROSS_GROUP_FOREMAN_RECIPIENT])
        );

        let mut explicit = json!({
            "group_id":"g_local","dst_group_id":"g_remote","to":["peer"]
        })
        .as_object()
        .cloned()
        .expect("explicit args");
        apply_cross_group_default(&mut explicit).expect("explicit");
        assert_eq!(explicit["to"], json!(["peer"]));

        let mut reply = json!({
            "group_id":"g_local","dst_group_id":"g_remote",
            "reply_to":"remote-event","to":["@peer"]
        })
        .as_object()
        .cloned()
        .expect("reply args");
        apply_cross_group_default(&mut reply).expect("cross-group reply");
        assert_eq!(reply["to"], json!(["@peer"]));
        assert_eq!(reply["reply_to"], "remote-event");

        let mut local = json!({"group_id":"g_local","dst_group_id":"g_local"})
            .as_object()
            .cloned()
            .expect("local args");
        apply_cross_group_default(&mut local).expect("local");
        assert!(local.get("to").is_none());

        for invalid in [json!(null), json!([]), json!([" "]), json!(7)] {
            let mut args = json!({
                "group_id":"g_local","dst_group_id":"g_remote","to":invalid
            })
            .as_object()
            .cloned()
            .expect("invalid args");
            assert!(apply_cross_group_default(&mut args).is_err());
        }
    }

    #[tokio::test]
    async fn remote_message_prefers_live_daemon_session_over_a_complete_direct_route() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let group = source_group_with_helper(&home);
        seed_bridge(
            &home,
            json!({"trusts":[{
                "trust_id":"trust-live-session","registration_id":"session-registration","group_id":group.group_id,
                "remote_group_id":"g_remote","remote_peer_id":"peer-remote",
                "remote_endpoint":"https://direct.example.invalid",
                "credential":"direct-credential",
                "remote_access_level":"messages","status":"active"
            }]}),
        );
        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&home).await;
        let client = DaemonClient::new(home.clone());
        let route = json!({"group_id":group.group_id,"remote_group_id":"g_remote","remote_peer_id":"peer-remote"});
        let opened = daemon(
            &client,
            "group_bridge_session_open",
            route.as_object().cloned().expect("route"),
        )
        .await
        .expect("open");
        let generation = opened["generation"]
            .as_str()
            .expect("generation")
            .to_owned();

        let send_home = home.clone();
        let send_client = client.clone();
        let group_id = group.group_id.clone();
        let send_task = tokio::spawn(async move {
            try_send(
                &send_home,
                &send_client,
                json!({
                    "group_id":group_id,"by":"helper","dst_group_id":"g_remote",
                    "to":["user"],"text":"through the live reverse session",
                    "message_mode":"send",
                    "insight":"The live route must preserve the same peer perspective as HTTP delivery."
                })
                .as_object()
                .cloned()
                .expect("send args"),
            )
            .await
        });
        let mut poll_args = route.as_object().cloned().expect("poll route");
        poll_args.insert("generation".into(), json!(generation));
        poll_args.insert("timeout_ms".into(), json!(1_000));
        let polled = daemon(&client, "group_bridge_session_poll", poll_args)
            .await
            .expect("poll");
        let frame = polled
            .get("request")
            .filter(|request| request.is_object())
            .expect("live session must receive the remote_send request");
        assert_eq!(frame["op"], "remote_send");
        assert!(frame["payload"]["text"].as_str().is_some_and(|text| {
            text.starts_with("through the live reverse session")
                && text.contains(cccc_core::peer_insight::PEER_PERSPECTIVE_AGENT_LABEL)
                && text.contains(
                    "The live route must preserve the same peer perspective as HTTP delivery.",
                )
        }));
        assert!(frame["payload"].get("insight").is_none());
        assert_eq!(frame["payload"]["message_mode"], "send");
        let mut complete_args = route.as_object().cloned().expect("complete route");
        complete_args.insert("generation".into(), opened["generation"].clone());
        complete_args.insert("response_to".into(), frame["request_id"].clone());
        complete_args.insert(
            "result".into(),
            json!({"ok":true,"receipt":{"status":"sent","remote_event_id":"remote-session-event"}}),
        );
        daemon(&client, "group_bridge_session_complete", complete_args)
            .await
            .expect("complete");
        let result = send_task
            .await
            .expect("join")
            .expect("remote classification")
            .expect("session send");
        assert_eq!(
            result["structuredContent"]["receipt"]["remote_event_id"],
            "remote-session-event"
        );
        assert_eq!(
            result["structuredContent"]["transport"],
            "group_bridge_session"
        );
        let bridge = cccc_core::group_bridge_legacy::load(&home).expect("bridge receipts");
        assert!(
            bridge["deliveries"].as_array().is_some_and(|receipts| {
                receipts.iter().any(|receipt| {
                    receipt["idempotency_key"]
                        == result["structuredContent"]["receipt"]["idempotency_key"]
                        && receipt["status"] == "sent"
                })
            }),
            "MCP delivery must be committed by the daemon to the shared receipt store"
        );
        daemon_task.abort();
    }

    #[tokio::test]
    async fn remote_message_falls_back_to_http_after_live_session_failure() {
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
                                "status":"sent","remote_event_id":"remote-http-fallback"
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
        let group = source_group_with_helper(&home);
        seed_bridge(
            &home,
            json!({"trusts":[{
                "trust_id":"trust-http-fallback","registration_id":"session-registration","group_id":group.group_id,
                "remote_group_id":"g_remote","remote_peer_id":"peer-remote",
                "remote_endpoint":endpoint,"credential":"direct-credential",
                "remote_access_level":"messages","status":"active"
            }]}),
        );
        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&home).await;
        let client = DaemonClient::new(home.clone());
        let route = json!({"group_id":group.group_id,"remote_group_id":"g_remote","remote_peer_id":"peer-remote"});
        let opened = daemon(
            &client,
            "group_bridge_session_open",
            route.as_object().cloned().expect("route"),
        )
        .await
        .expect("open");

        let send_home = home.clone();
        let send_client = client.clone();
        let group_id = group.group_id.clone();
        let send_task = tokio::spawn(async move {
            try_send(
                &send_home,
                &send_client,
                json!({
                    "group_id":group_id,"by":"helper","dst_group_id":"g_remote",
                    "to":["user"],"text":"fall back after session failure","mode":"send"
                })
                .as_object()
                .cloned()
                .expect("send args"),
            )
            .await
        });
        let mut poll_args = route.as_object().cloned().expect("poll route");
        poll_args.insert("generation".into(), opened["generation"].clone());
        poll_args.insert("timeout_ms".into(), json!(1_000));
        let polled = daemon(&client, "group_bridge_session_poll", poll_args)
            .await
            .expect("poll");
        let frame = polled
            .get("request")
            .filter(|request| request.is_object())
            .expect("live session must receive the remote_send request");
        let mut complete_args = route.as_object().cloned().expect("complete route");
        complete_args.insert("generation".into(), opened["generation"].clone());
        complete_args.insert("response_to".into(), frame["request_id"].clone());
        complete_args.insert(
            "result".into(),
            json!({"ok":false,"error":{"code":"peer_busy","message":"retry over HTTP"}}),
        );
        daemon(&client, "group_bridge_session_complete", complete_args)
            .await
            .expect("complete");

        let result = send_task
            .await
            .expect("join")
            .expect("remote classification")
            .expect("HTTP fallback");
        assert_eq!(
            result["structuredContent"]["receipt"]["remote_event_id"],
            "remote-http-fallback"
        );
        assert_eq!(captured.lock().expect("capture").len(), 1);
        daemon_task.abort();
        remote_task.abort();
    }

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
                                "status":"sent","remote_event_id":"remote-reply",
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
        seed_bridge(
            &home,
            json!({"trusts":[{
                "trust_id":"trust-remote-reply","registration_id":"registration-1",
                "group_id":group.group_id,
                "remote_group_id":"g_remote",
                "remote_peer_id":"peer-remote",
                "remote_endpoint":endpoint,
                "credential":"secret",
                "transport":"group_bridge_session",
                "remote_access_level":"messages",
                "status":"active"
            }]}),
        );
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

        crate::router::call(&home, &client, "cccc_message_reply", args)
            .await
            .expect("relay reply through daemon");
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
        let local_replies = events
            .iter()
            .filter(|event| {
                event.kind == "chat.message"
                    && event.data.get("reply_to").and_then(Value::as_str)
                        == Some(inbound.id.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(local_replies.len(), 1);
        let local_reply = local_replies[0];
        assert_eq!(local_reply.data["reply_to"], inbound.id);
        assert_eq!(local_reply.data["to"], json!(["user"]));
        let projected_receipts = events
            .iter()
            .filter(|event| event.kind == "chat.cross_group_receipt")
            .collect::<Vec<_>>();
        assert_eq!(projected_receipts.len(), 1);
        assert_eq!(
            projected_receipts[0].data["source_event_id"],
            local_reply.id
        );
        assert_eq!(
            projected_receipts[0].data["remote_event_id"],
            "remote-reply"
        );

        daemon_task.abort();
        remote_task.abort();
    }

    #[tokio::test]
    async fn remote_reply_uses_live_reverse_session_without_group_lock_deadlock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("receiver", "").expect("group");
        seed_bridge(
            &home,
            json!({"trusts":[{
                "trust_id":"trust-reverse-session",
                "registration_id":"registration-session",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer-remote","transport":"group_bridge_session",
                "remote_access_level":"messages","status":"active"
            }]}),
        );
        let mut inbound = Event::new("chat.message", &group.group_id);
        inbound.by = "group_bridge:peer-remote".into();
        inbound.data = json!({
            "text":"question","to":["user"],
            "source_platform":"group_bridge_session",
            "source_user_id":"peer-remote","src_group_id":"g_remote",
            "src_event_id":"remote-origin-event","src_by":"original-agent",
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
        let route = json!({
            "group_id":group.group_id,"remote_group_id":"g_remote",
            "remote_peer_id":"peer-remote"
        });
        let opened = daemon(
            &client,
            "group_bridge_session_open",
            route.as_object().cloned().expect("route"),
        )
        .await
        .expect("open");

        let reply_home = home.clone();
        let reply_client = client.clone();
        let group_id = group.group_id.clone();
        let inbound_id = inbound.id.clone();
        let reply_task = tokio::spawn(async move {
            crate::router::call(
                &reply_home,
                &reply_client,
                "cccc_message_reply",
                json!({
                    "group_id":group_id,"by":"user","reply_to":inbound_id,
                    "text":"answer through reverse session",
                    "insight":"Keeping the reply on the original remote thread avoids losing context."
                })
                .as_object()
                .cloned()
                .expect("reply args"),
            )
            .await
        });

        let mut poll_args = route.as_object().cloned().expect("poll route");
        poll_args.insert("generation".into(), opened["generation"].clone());
        poll_args.insert("timeout_ms".into(), json!(1_000));
        let polled = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            daemon(&client, "group_bridge_session_poll", poll_args),
        )
        .await
        .expect("poll must not wait for the reply group lock")
        .expect("poll");
        let frame = &polled["request"];
        assert_eq!(frame["payload"]["to"], json!(["original-agent"]));
        assert_eq!(frame["payload"]["reply_to"], "remote-origin-event");

        let mut complete_args = route.as_object().cloned().expect("complete route");
        complete_args.insert("generation".into(), opened["generation"].clone());
        complete_args.insert("response_to".into(), frame["request_id"].clone());
        complete_args.insert(
            "result".into(),
            json!({"ok":true,"event_id":"remote-session-reply"}),
        );
        daemon(&client, "group_bridge_session_complete", complete_args)
            .await
            .expect("complete");
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), reply_task)
            .await
            .expect("reply must complete after the session response")
            .expect("reply task")
            .expect("reply");
        assert_eq!(
            result["structuredContent"]["group_bridge_reply"]["receipt"]["remote_event_id"],
            "remote-session-reply"
        );
        assert_eq!(
            ledger::read_all(&ledger_path)
                .expect("events")
                .into_iter()
                .filter(|event| event.kind == "chat.message")
                .count(),
            2
        );
        daemon_task.abort();
    }

    #[tokio::test]
    async fn trusted_read_route_is_selected_for_remote_message_delivery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let group = source_group_with_helper(&home);
        seed_bridge(
            &home,
            json!({
                "trusts":[{
                    "trust_id":"trust-read-route",
                    "group_id":group.group_id,
                    "remote_group_id":"g_remote",
                    "remote_endpoint":"http://127.0.0.1:9",
                    "credential":"secret",
                    "remote_access_level":"read",
                    "status":"active"
                }]
            }),
        );
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
        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&home).await;

        let result = try_send(&home, &client, args)
            .await
            .expect("trusted remote route")
            .expect("durable retry receipt");
        assert_eq!(result["structuredContent"]["receipt"]["status"], "retrying");
        let bridge = cccc_core::group_bridge_legacy::load(&home).expect("bridge receipts");
        assert!(bridge["deliveries"].as_array().is_some_and(|receipts| {
            receipts
                .iter()
                .any(|receipt| receipt["status"] == "retrying")
        }));
        daemon_task.abort();
    }

    #[tokio::test]
    async fn remote_reply_without_active_route_returns_a_specific_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("receiver", "").expect("group");
        let mut inbound = Event::new("chat.message", &group.group_id);
        inbound.by = "group_bridge:peer-missing".into();
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

        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&home).await;
        let error = crate::router::call(
            &home,
            &DaemonClient::new(home.clone()),
            "cccc_message_reply",
            args,
        )
        .await
        .expect_err("missing route");
        assert!(error.contains("group_bridge_reply_route_not_found"));
        daemon_task.abort();
    }

    #[tokio::test]
    async fn remote_reply_does_not_match_trust_without_remote_peer_id() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("receiver", "").expect("group");
        seed_bridge(
            &home,
            json!({"trusts":[{
                "trust_id":"trust-missing-peer",
                "group_id":group.group_id,
                "remote_group_id":"g_remote",
                "remote_endpoint":"http://127.0.0.1:9",
                "credential":"secret",
                "transport":"group_bridge_session",
                "remote_access_level":"messages",
                "status":"active"
            }]}),
        );
        let mut inbound = Event::new("chat.message", &group.group_id);
        inbound.by = "group_bridge:peer-remote".into();
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

        let daemon_home = home.clone();
        let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        wait_for_daemon(&home).await;
        let error = crate::router::call(
            &home,
            &DaemonClient::new(home.clone()),
            "cccc_message_reply",
            args,
        )
        .await
        .expect_err("trust without peer must not authorize reply");
        assert!(error.contains("group_bridge_reply_route_not_found"));
        daemon_task.abort();
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
            json!({"to":["@foreman"],"mode":"request_reply"}),
            json!({"to":["peer1"],"mode":"later"}),
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
