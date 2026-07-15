#![cfg(unix)]

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn duplicate_client_id_returns_the_original_event() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = call(&home, "group_create", json!({"title":"idempotency"}));
    let group_id = group.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let args = json!({
        "group_id":group_id,
        "by":"user",
        "to":[],
        "text":"only once",
        "client_id":"client-1"
    });

    let first = call(&home, "send", args.clone());
    let second = call(&home, "send", args);
    let tail = call(
        &home,
        "ledger_tail",
        json!({"group_id":group_id,"kind":"chat","limit":20}),
    );

    assert_eq!(first.result["event"]["id"], second.result["event"]["id"]);
    assert_eq!(second.result["duplicate"], true);
    assert_eq!(tail.result["events"].as_array().map(Vec::len), Some(1));
}

#[test]
fn starting_an_offline_actor_replays_its_unread_message() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = call(&home, "group_create", json!({"title":"offline replay"}));
    let group_id = group.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runtime":"custom",
            "runner":"pty",
            "submit":"newline",
            "command":["sh","-c","stty -echo; IFS= read -r line; printf 'RECEIVED:%s' \"$line\"; sleep 2"],
            "enabled":false,
            "by":"user"
        }),
    );
    let sent = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake delivery"}),
    );
    assert_eq!(sent.result["delivery"]["state"], "inbox");

    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    let mut output = String::new();
    while std::time::Instant::now() < deadline {
        let tail = call(
            &home,
            "terminal_tail",
            json!({"group_id":group_id,"actor_id":"peer1","max_chars":4000,"by":"user"}),
        );
        output = tail.result["text"].as_str().unwrap_or_default().to_owned();
        if output.contains("RECEIVED:[cccc] user → peer1: wake delivery") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(output.contains("RECEIVED:[cccc] user → peer1: wake delivery"));
    let _ = call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
}

#[test]
fn offline_replay_drains_more_than_the_delivery_queue_capacity() {
    const MESSAGE_COUNT: usize = 300;
    const PAYLOAD: &str = "[cccc] user → peer1: queued";
    let expected_bytes = PAYLOAD.len() * MESSAGE_COUNT;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = call(
        &home,
        "group_create",
        json!({"title":"large offline replay"}),
    );
    let group_id = group.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runtime":"custom",
            "runner":"pty",
            "submit":"none",
            "command":["sh","-c",format!("stty raw -echo; dd bs=1 count={expected_bytes} of=/dev/null 2>/dev/null; printf 'COUNT:{MESSAGE_COUNT}'; sleep 2")],
            "enabled":false,
            "by":"user"
        }),
    );
    for index in 0..MESSAGE_COUNT {
        call(
            &home,
            "send",
            json!({
                "group_id":group_id,
                "by":"user",
                "to":["peer1"],
                "text":"queued",
                "client_id":format!("queued-{index}")
            }),
        );
    }
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut output = String::new();
    while std::time::Instant::now() < deadline {
        let tail = call(
            &home,
            "terminal_tail",
            json!({"group_id":group_id,"actor_id":"peer1","max_chars":4000,"by":"user"}),
        );
        output = tail.result["text"].as_str().unwrap_or_default().to_owned();
        if output.contains("COUNT:300") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(output.contains("COUNT:300"), "terminal output: {output}");

    let inbox = call(
        &home,
        "inbox_list",
        json!({"group_id":group_id,"actor_id":"peer1","limit":1000,"by":"user"}),
    );
    assert_eq!(inbox.result["messages"].as_array().map(Vec::len), Some(0));
    let _ = call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}
