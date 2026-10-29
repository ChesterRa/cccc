//! A question to the user through the real MCP admission path and a real
//! daemon: the tool keeps the structure, the daemon generates the text, and a
//! question cannot ride on a reply. No provider or terminal is started.
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};

async fn send(home: &HomeLayout, group_id: &str, arguments: Value) -> Value {
    cccc_mcp::handle_request_for_actor(
        home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"cccc_message_send","arguments":arguments}}),
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
    let asked = send(
        &home,
        group_id,
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

    let on_reply = send(
        &home,
        group_id,
        json!({"event_id": event["id"], "to": "user", "text": "again", "question": question}),
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
