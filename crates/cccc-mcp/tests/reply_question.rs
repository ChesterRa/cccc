//! A question to the user through the real MCP admission path and a real
//! daemon: the tool keeps the structure, the daemon generates the text, and a
//! question cannot ride on a reply. No provider or terminal is started.
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};

async fn call(home: &HomeLayout, group_id: &str, tool: &str, arguments: Value) -> Value {
    cccc_mcp::handle_request_for_actor(
        home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":tool,"arguments":arguments}}),
        group_id,
        "lead",
    )
    .await["result"]
        .clone()
}

#[tokio::test]
async fn an_actor_asks_the_user_a_question_through_the_send_tool() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("reply question", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("lead")).expect("actor");
    let mut web = cccc_contracts::Actor::new("web1");
    web.runtime = cccc_contracts::ActorRuntime::GrokWebModel;
    cccc_core::actors::add(&mut group, web).expect("web model");
    store.save(&group).expect("save");
    let group_id = group.group_id.as_str();
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    let ping = DaemonRequest {
        v: 1,
        op: "ping".into(),
        args: Default::default(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while client.call(&ping).await.is_err() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");

    let question = json!({
        "v": 1,
        "prompt": "Ship the fix today?",
        "options": [{"label": "Ship today"}, {"label": "Wait for review", "consequence": "one day later"}],
        "recommended": {"option_id": "B", "reason": "the change touches deletion"}
    });
    let asked = call(
        &home,
        group_id,
        "cccc_message_send",
        json!({"to": "user", "mode": "request_reply", "text": "Tests pass.", "question": question}),
    )
    .await;
    assert_ne!(asked["isError"], true, "{asked}");
    let event = &asked["structuredContent"]["event"];
    assert_eq!(event["data"]["question"]["options"][1]["id"], "B");
    assert_eq!(
        event["data"]["text"],
        "Tests pass.\n\nShip the fix today?\n\nA. Ship today\nB. Wait for review — one day later\n\
         Recommended: B — the change touches deletion\n\nReply with A, B, or your own answer."
    );
    assert!(
        asked["structuredContent"]
            .get("mcp_omitted_fields")
            .is_none()
    );

    let sent = call(
        &home,
        group_id,
        "cccc_message_send",
        json!({"to":"user","mode":"send","text":"plain update","insight":"The full result is in the ledger."}),
    )
    .await;
    let projected = &sent["structuredContent"];
    assert!(projected["event"]["data"].get("text").is_none());
    assert!(projected["event"]["data"].get("insight").is_none());
    assert_eq!(
        projected["mcp_omitted_fields"]["event"],
        json!(["text", "insight"])
    );
    let text_payload: Value =
        serde_json::from_str(sent["content"][0]["text"].as_str().expect("text content"))
            .expect("JSON payload");
    assert_eq!(&text_payload, projected);
    let original = client
        .call(&DaemonRequest {
            v: 1,
            op: "message_history".into(),
            args: json!({"group_id":group_id,"actor_id":"lead","by":"lead",
                    "event_id":projected["event"]["id"]})
            .as_object()
            .expect("arguments")
            .clone(),
        })
        .await
        .expect("ledger read");
    assert!(original.ok);
    assert_eq!(
        original.result["messages"][0]["data"]["text"],
        "plain update"
    );
    assert_eq!(
        original.result["messages"][0]["data"]["insight"],
        "The full result is in the ledger."
    );

    let web_result = cccc_mcp::handle_request_for_actor(
        &home,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"cccc_message_send","arguments":{"to":"user","mode":"send","text":"web update"}}}),
        group_id,
        "web1",
    ).await;
    let web_payload = &web_result["result"]["structuredContent"];
    assert_eq!(web_payload["event"]["by"], "web1");
    assert!(web_payload["event"]["data"].get("text").is_none());
    assert_eq!(web_payload["mcp_omitted_fields"]["event"], json!(["text"]));

    // A send starts a new message, so it takes no existing event at all.
    let send_on_event = call(
        &home,
        group_id,
        "cccc_message_send",
        json!({"event_id": event["id"], "to": "user", "text": "again", "question": question}),
    )
    .await;
    assert_eq!(send_on_event["isError"], true, "{send_on_event}");
    assert!(
        send_on_event.to_string().contains("cccc_message_reply"),
        "{send_on_event}"
    );

    let on_reply = call(
        &home,
        group_id,
        "cccc_message_reply",
        json!({"event_id": event["id"], "text": "again", "question": question}),
    )
    .await;
    assert_eq!(on_reply["isError"], true, "{on_reply}");
    assert!(
        on_reply.to_string().contains("invalid_question"),
        "{on_reply}"
    );

    daemon.abort();
    let _ = daemon.await;
}
