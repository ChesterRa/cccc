#![cfg(unix)]

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

#[test]
fn serializes_delivery_notifies_and_advances_cursor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(
        &home,
        "group_create",
        json!({"title":"message-delivery-test","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runner":"pty",
            "runtime":"custom",
            "submit":"newline",
            "command":["sh","-c","stty -echo; IFS= read -r first; IFS= read -r second; IFS= read -r third; printf 'FIRST:%s\\nSECOND:%s\\nTHIRD:%s' \"$first\" \"$second\" \"$third\"; sleep 2"],
            "by":"user"
        }),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );

    let first = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"one"}),
    );
    let second = call(
        &home,
        "tracked_send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"two"}),
    );
    let notify = call(
        &home,
        "system_notify",
        json!({"group_id":group_id,"by":"system","to":["peer1"],"text":"notice"}),
    );
    assert_eq!(first.result["delivery"]["state"], "queued");
    assert_eq!(second.result["delivery"]["queued"], 1);
    assert_eq!(notify.result["delivery"]["state"], "queued");

    wait_for(&home, &group_id, "FIRST:[cccc] user → peer1: one");
    let tail = call(
        &home,
        "terminal_tail",
        json!({"group_id":group_id,"actor_id":"peer1"}),
    );
    let text = tail.result["text"].as_str().unwrap_or_default();
    assert!(text.contains("SECOND:[cccc] user → peer1: two"));
    assert!(text.contains("THIRD:[cccc] system → peer1: notice"));

    let inbox = call(
        &home,
        "inbox_list",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    assert_eq!(inbox.result["messages"].as_array().map(Vec::len), Some(0));
}

fn wait_for(home: &HomeLayout, group_id: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        let tail = call(
            home,
            "terminal_tail",
            json!({"group_id":group_id,"actor_id":"peer1"}),
        );
        if tail.result["text"]
            .as_str()
            .unwrap_or_default()
            .contains(expected)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "PTY did not receive {expected:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    };
    let response = cccc_daemon::handle_request(home, &request);
    assert!(
        response.ok,
        "{op} failed: {:?}",
        response.error.as_ref().map(|error| &error.message)
    );
    response
}
