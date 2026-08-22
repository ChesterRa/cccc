use super::*;

pub(super) async fn complete_web_delivery_over_session(
    address: &std::net::SocketAddr,
    home: &HomeLayout,
    socket: &mut TestSocket,
    group_id: &str,
) {
    let url = format!("http://{address}/api/v1/groups/{group_id}/send_cross_group");
    let client = reqwest::Client::new();
    let request_client = client.clone();
    let request_url = url.clone();
    let request = tokio::spawn(async move {
        request_client
            .post(request_url)
            .json(&json!({
                "dst_group_id":"g_sender",
                "text":"web over reverse session",
                "message_mode":"send",
                "to":["@foreman"],
                "client_id":"web-session-once",
                "remote_reply_to_event_id":"remote-parent-event"
            }))
            .send()
            .await
            .expect("web send")
    });
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), next_socket_json(socket))
        .await
        .expect("Web cross-group send did not use the live daemon session");
    assert_eq!(frame["op"], "remote_send");
    assert_eq!(frame["payload"]["reply_to"], "remote-parent-event");
    let source_ledger = GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(group_id))
        .expect("source ledger");
    let source_messages = ledger::read_all(&source_ledger)
        .expect("source events before remote completion")
        .into_iter()
        .filter(|event| {
            event.kind == "chat.message"
                && event.data.get("dst_group_id").and_then(Value::as_str) == Some("g_sender")
                && event.data.get("text").and_then(Value::as_str)
                    == Some("web over reverse session")
        })
        .collect::<Vec<_>>();
    assert_eq!(source_messages.len(), 1);
    assert!(source_messages[0].data.get("source_by").is_none());
    assert!(source_messages[0].data.get("transport").is_none());
    assert_eq!(
        frame["payload"]["src_event_id"], source_messages[0].id,
        "the local source event must be durable before remote delivery"
    );
    socket
        .send(WsMessage::Text(
            json!({
                "type":"response",
                "response_to":frame["request_id"],
                "result":{"ok":true,"receipt":{
                    "status":"sent","event_id":"remote-web-session"
                }}
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("web response");
    let response = request.await.expect("web request join");
    let status = response.status();
    let body = response.json::<Value>().await.expect("web body");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["receipt"]["status"], "sent");
    assert_eq!(
        body["result"]["receipt"]["remote_event_id"],
        "remote-web-session"
    );
    let source_event = body["result"]["source_event"].clone();
    assert!(source_event["id"].is_string(), "{body}");
    let retry = client
        .post(url)
        .json(&json!({
            "dst_group_id":"g_sender",
            "text":"web over reverse session",
            "message_mode":"send",
            "to":["@foreman"],
            "client_id":"web-session-once",
            "remote_reply_to_event_id":"remote-parent-event"
        }))
        .send()
        .await
        .expect("retry web send");
    let retry_body = retry.json::<Value>().await.expect("retry web body");
    assert_eq!(retry_body["result"]["deduped"], true, "{retry_body}");
    assert_eq!(
        retry_body["result"]["source_event"], source_event,
        "{retry_body}"
    );
    let bridge = cccc_core::group_bridge_legacy::load(home).expect("bridge receipts");
    assert!(bridge["deliveries"].as_array().is_some_and(|receipts| {
        receipts.iter().any(|receipt| {
            receipt["idempotency_key"] == "web-session-once" && receipt["status"] == "sent"
        })
    }));
    let events = ledger::read_all(&source_ledger).expect("source events after retry");
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.kind == "chat.message"
                    && event.data.get("dst_group_id").and_then(Value::as_str) == Some("g_sender")
                    && event.data.get("text").and_then(Value::as_str)
                        == Some("web over reverse session")
            })
            .count(),
        1,
        "idempotent retries must not duplicate the source message"
    );
    let receipts = events
        .iter()
        .filter(|event| {
            event.kind == "chat.cross_group_receipt"
                && event.data.get("idempotency_key").and_then(Value::as_str)
                    == Some("web-session-once")
        })
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), 1, "the delivery receipt must project once");
    assert_eq!(receipts[0].data["source_event_id"], source_messages[0].id);
    assert_eq!(receipts[0].data["remote_event_id"], "remote-web-session");
}
