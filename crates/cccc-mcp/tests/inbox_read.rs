use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

#[tokio::test]
async fn core_inbox_read_consumes_native_daemon_batches() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("inbox read", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("actor");
    store.save(&group).expect("save group");

    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;

    let first_event_id = send_mail(&client, &group.group_id, "first").await;
    let second_event_id = send_mail(&client, &group.group_id, "second").await;
    let direct_event_id = send_message(&client, &group.group_id, "direct", "send").await;

    let first = mcp_call(
        &home,
        &group.group_id,
        1,
        "cccc_inbox_read",
        json!({"limit":1}),
    )
    .await;
    assert!(first.get("error").is_none(), "first read failed: {first}");
    assert_eq!(
        first["result"]["structuredContent"]["messages"][0]["id"],
        first_event_id
    );
    assert_eq!(
        first["result"]["structuredContent"]["cursor"]["event_id"],
        first_event_id
    );
    assert_eq!(
        first["result"]["structuredContent"]["event"]["kind"],
        "mail.read"
    );

    let second = mcp_call(
        &home,
        &group.group_id,
        2,
        "cccc_inbox_read",
        json!({"limit":50}),
    )
    .await;
    assert_eq!(
        second["result"]["structuredContent"]["messages"][0]["id"],
        second_event_id
    );
    assert_eq!(
        second["result"]["structuredContent"]["cursor"]["event_id"],
        second_event_id
    );

    let empty = mcp_call(&home, &group.group_id, 3, "cccc_inbox_read", json!({})).await;
    assert_eq!(empty["result"]["structuredContent"]["messages"], json!([]));
    assert_eq!(empty["result"]["structuredContent"]["event"], Value::Null);

    let history = mcp_call(
        &home,
        &group.group_id,
        4,
        "cccc_message_history",
        json!({"mode":"send"}),
    )
    .await;
    assert_eq!(
        history["result"]["structuredContent"]["messages"][0]["id"],
        direct_event_id
    );

    daemon_task.abort();
}

#[tokio::test]
async fn file_send_uses_daemon_owned_preflight_before_blob_storage() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let scope_root = temp.path().join("scope");
    std::fs::create_dir_all(&scope_root).expect("scope");
    std::fs::write(scope_root.join("note.txt"), "deliverable").expect("file");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("file send", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("actor");
    store.save(&group).expect("save group");
    cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::Scope {
            scope_key: "scope_project".into(),
            url: scope_root.to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach scope");

    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;
    let blobs_dir = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("blobs");

    let rejected = mcp_call(
        &home,
        &group.group_id,
        10,
        "cccc_file",
        json!({"action":"send","path":"note.txt","to":["user"],"mode":"mail"}),
    )
    .await;
    assert_eq!(rejected["result"]["isError"], true, "{rejected}");
    assert!(
        !blobs_dir.exists()
            || std::fs::read_dir(&blobs_dir)
                .expect("blobs")
                .next()
                .is_none()
    );

    let sent = mcp_call(
        &home,
        &group.group_id,
        11,
        "cccc_file",
        json!({
            "action":"send","path":"note.txt","text":"attached",
            "to":["user"],"mode":"send"
        }),
    )
    .await;
    assert!(sent.get("error").is_none(), "send failed: {sent}");
    assert_eq!(sent["result"]["structuredContent"]["sent"], true);
    assert_eq!(
        sent["result"]["structuredContent"]["result"]["event"]["data"]["message_mode"],
        "send"
    );
    assert_eq!(
        std::fs::read_dir(&blobs_dir)
            .expect("blobs")
            .filter_map(Result::ok)
            .count(),
        1
    );

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

async fn send_mail(client: &DaemonClient, group_id: &str, text: &str) -> String {
    send_message(client, group_id, text, "mail").await
}

async fn send_message(client: &DaemonClient, group_id: &str, text: &str, mode: &str) -> String {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: "send".into(),
            args: json!({
                "group_id":group_id,
                "by":"user",
                "to":["peer1"],
                "text":text,
                "message_mode":mode,
            })
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
