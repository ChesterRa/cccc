#![cfg(unix)]

// Included by the crate-level integration test harness.

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
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"lead","runner":"headless","by":"user"}),
    );
    let args = json!({
        "group_id":group_id,
        "by":"user",
        "to":["lead"],
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
fn directed_message_auto_wakes_an_offline_actor_exactly_once() {
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
            "command":["sh","-c","stty -echo; IFS= read -r preamble; IFS= read -r message; printf 'PREAMBLE:%s\\nRECEIVED:%s' \"$preamble\" \"$message\"; sleep 2"],
            "enabled":false,
            "by":"user"
        }),
    );
    let sent = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake delivery"}),
    );
    assert_eq!(sent.result["delivery"]["state"], "queued");
    assert_eq!(sent.result["delivery"]["online"], 0);

    let output = wait_for_terminal(
        &home,
        group_id,
        "RECEIVED:[cccc] user → peer1: wake delivery",
    );
    assert_eq!(
        output
            .matches("RECEIVED:[cccc] user → peer1: wake delivery")
            .count(),
        1
    );
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(actors.result["actors"][0]["enabled"], true);
    assert_eq!(actors.result["actors"][0]["running"], true);
    let _ = call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
}

fn wait_for_terminal(home: &HomeLayout, group_id: &str, expected: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let response = cccc_daemon::handle_request(
            home,
            &DaemonRequest {
                v: 1,
                op: "terminal_tail".into(),
                args: json!({"group_id":group_id,"actor_id":"peer1","max_chars":4000,"by":"user"})
                    .as_object()
                    .cloned()
                    .unwrap_or_else(Map::new),
            },
        );
        let output = response
            .result
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if response.ok && output.contains(expected) {
            return output.to_owned();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PTY did not receive {expected:?}; response={response:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
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
