#![cfg(unix)]

// Included by the crate-level integration test harness.

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, ledger};
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
        "message_mode":"send",
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
fn directed_message_wakes_an_explicitly_stopped_actor_and_delivers_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = call(&home, "group_create", json!({"title":"offline replay"}));
    let group_id = group.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":project,"by":"user"}),
    );
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
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"wake delivery","message_mode":"send"}),
    );
    assert_eq!(sent.result["message_mode"], "send");
    let sent_event_id = sent.result["event"]["id"].as_str().expect("sent event id");
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(actors.result["actors"][0]["enabled"], true);

    assert_eq!(
        wait_for_accepted_delivery(&home, group_id, sent_event_id),
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

fn wait_for_accepted_delivery(home: &HomeLayout, group_id: &str, source_event_id: &str) -> usize {
    let ledger_path = GroupStore::new(home.clone())
        .expect("store")
        .ledger_path(group_id)
        .expect("ledger path");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let accepted = ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .filter(|event| {
                event.kind == "runtime.delivery"
                    && event.data["source_event_id"] == source_event_id
                    && event.data["actor_id"] == "peer1"
                    && event.data["state"] == "accepted"
            })
            .count();
        if accepted > 0 {
            return accepted;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "runtime did not accept delivery for {source_event_id}"
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
