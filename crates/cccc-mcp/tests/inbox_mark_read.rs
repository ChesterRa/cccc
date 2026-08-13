use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

#[tokio::test]
async fn core_inbox_mark_read_tool_calls_native_daemon_operations() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("inbox mark read", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("actor");
    store.save(&group).expect("save group");

    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;

    let first_event_id = send_message(&client, &group.group_id, "first").await;
    let marked = mcp_call(
        &home,
        &group.group_id,
        1,
        "cccc_inbox_mark_read",
        json!({"action":"read","event_id":first_event_id}),
    )
    .await;
    assert!(marked.get("error").is_none(), "mark read failed: {marked}");
    assert_eq!(
        marked["result"]["structuredContent"]["cursor"]["event_id"],
        first_event_id
    );

    let second_event_id = send_message(&client, &group.group_id, "second").await;
    let marked_all = mcp_call(
        &home,
        &group.group_id,
        2,
        "cccc_inbox_mark_read",
        json!({"action":"read_all","kind_filter":"all"}),
    )
    .await;
    assert!(
        marked_all.get("error").is_none(),
        "mark all read failed: {marked_all}"
    );
    assert_eq!(
        marked_all["result"]["structuredContent"]["cursor"]["event_id"],
        second_event_id
    );

    let inbox = mcp_call(
        &home,
        &group.group_id,
        3,
        "cccc_inbox_list",
        json!({"kind_filter":"all"}),
    )
    .await;
    assert_eq!(inbox["result"]["structuredContent"]["messages"], json!([]));

    daemon_task.abort();
}

async fn wait_for_daemon(client: &DaemonClient) {
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

async fn send_message(client: &DaemonClient, group_id: &str, text: &str) -> String {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: "send".into(),
            args: json!({"group_id":group_id,"by":"user","to":["peer1"],"text":text})
                .as_object()
                .cloned()
                .expect("send args"),
        })
        .await
        .expect("send message");
    assert!(response.ok, "send message: {:?}", response.error);
    response.result["event"]["id"]
        .as_str()
        .expect("event id")
        .to_owned()
}

async fn mcp_call(
    home: &HomeLayout,
    group_id: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    cccc_mcp::handle_request_for_actor(
        home,
        &json!({
            "jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
        group_id,
        "peer1",
    )
    .await
}
