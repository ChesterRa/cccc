use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};
use std::time::Duration;

fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("object arguments").clone(),
    }
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(home, &request(op, args))
}

fn event(group_id: &str, text: &str) -> Event {
    let mut event = Event::new("chat.message", group_id);
    event.by = "worker".into();
    event.data = json!({"text":text,"to":["user"],"message_mode":"request_reply"})
        .as_object()
        .expect("message data")
        .clone();
    event
}

fn fixture() -> (tempfile::TempDir, HomeLayout, GroupStore, String) {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("group store");
    let group = groups.create("Pin fixture", "").expect("group");
    (temp, home, groups, group.group_id)
}

#[test]
fn message_pins_follow_ledger_order_keep_status_and_do_not_change_the_ledger() {
    let (_temp, home, groups, group_id) = fixture();
    let path = groups.ledger_path(&group_id).expect("ledger path");
    let first = event(&group_id, "first message");
    let second = event(&group_id, "second message");
    ledger::append(&path, &first).expect("first event");
    ledger::append(&path, &second).expect("second event");
    let before = std::fs::read(&path).expect("ledger before");

    for id in [&second.id, &first.id] {
        let result = call(
            &home,
            "message_pins_set",
            json!({"group_id":group_id,"event_id":id,"pinned":true,"detail":"ids"}),
        );
        assert!(result.ok, "{:?}", result.error);
    }
    let ids = call(
        &home,
        "message_pins_list",
        json!({"group_id":group_id,"detail":"ids"}),
    );
    assert!(ids.ok, "{:?}", ids.error);
    assert_eq!(ids.result["pins"][0]["event_id"], first.id);
    assert_eq!(ids.result["pins"][1]["event_id"], second.id);
    assert!(ids.result["pins"][0].get("event").is_none());
    let pinned_at = ids.result["pins"][0]["pinned_at"].clone();

    let duplicate = call(
        &home,
        "message_pins_set",
        json!({"group_id":group_id,"event_id":first.id,"pinned":true}),
    );
    assert!(duplicate.ok, "{:?}", duplicate.error);
    assert_eq!(duplicate.result["pins"][0]["pinned_at"], pinned_at);
    assert_eq!(
        duplicate.result["pins"][0]["event"]["data"]["text"],
        "first message"
    );
    assert_eq!(
        duplicate.result["pins"][0]["event"]["_obligation_status"]["user"]["reply_requested"],
        true
    );
    assert_eq!(std::fs::read(&path).expect("ledger after"), before);

    for _ in 0..2 {
        let removed = call(
            &home,
            "message_pins_set",
            json!({"group_id":group_id,"event_id":first.id,"pinned":false,"detail":"ids"}),
        );
        assert!(removed.ok, "{:?}", removed.error);
        assert_eq!(removed.result["pins"].as_array().expect("pins").len(), 1);
        assert_eq!(removed.result["pins"][0]["event_id"], second.id);
    }
}

#[test]
fn message_pins_only_allow_users_and_group_message_targets() {
    let (_temp, home, groups, group_id) = fixture();
    let other = groups.create("other group", "").expect("other group");
    let foreign = event(&other.group_id, "foreign");
    ledger::append(
        &groups.ledger_path(&other.group_id).expect("other ledger"),
        &foreign,
    )
    .expect("foreign event");
    let mut notice = Event::new("system.notify", &group_id);
    notice.by = "system".into();
    ledger::append(&groups.ledger_path(&group_id).expect("ledger"), &notice).expect("notice");

    for id in [&foreign.id, &notice.id] {
        let result = call(
            &home,
            "message_pins_set",
            json!({"group_id":group_id,"event_id":id,"pinned":true}),
        );
        assert_eq!(result.error.expect("rejected").code, "pin_target_not_found");
    }
    for by in [json!("worker"), Value::Null] {
        for op in ["message_pins_list", "message_pins_set"] {
            let result = call(
                &home,
                op,
                json!({"group_id":group_id,"by":by,"event_id":notice.id,"pinned":false}),
            );
            assert_eq!(result.error.expect("rejected").code, "permission_denied");
        }
    }
    let invalid = call(
        &home,
        "message_pins_set",
        json!({"group_id":group_id,"event_id":notice.id,"pinned":"false"}),
    );
    assert_eq!(invalid.error.expect("rejected").code, "invalid_args");
}

#[test]
fn message_pins_keep_unavailable_messages_removable() {
    let (_temp, home, groups, group_id) = fixture();
    let state = groups
        .group_dir(&group_id)
        .expect("group dir")
        .join("state/message_pins/pins.json");
    cccc_core::fs::write_json(
        &state,
        &json!({"v":1,"pins":[{"event_id":"missing","pinned_at":"2026-01-01T00:00:00Z"}]}),
    )
    .expect("stale pin");
    let listed = call(&home, "message_pins_list", json!({"group_id":group_id}));
    assert!(listed.ok, "{:?}", listed.error);
    assert_eq!(listed.result["pins"][0]["event_id"], "missing");
    assert!(listed.result["pins"][0]["event"].is_null());

    let removed = call(
        &home,
        "message_pins_set",
        json!({"group_id":group_id,"event_id":"missing","pinned":false}),
    );
    assert!(removed.ok, "{:?}", removed.error);
    assert_eq!(removed.result["pins"], json!([]));
}

#[test]
fn message_pins_report_invalid_state_without_overwriting_it() {
    let (_temp, home, groups, group_id) = fixture();
    let state = groups
        .group_dir(&group_id)
        .expect("group dir")
        .join("state/message_pins/pins.json");
    let invalid = json!({"v":1,"pins":[
        {"event_id":"same","pinned_at":"2026-01-01T00:00:00Z"},
        {"event_id":"same","pinned_at":"2026-01-02T00:00:00Z"}
    ]});
    cccc_core::fs::write_json(&state, &invalid).expect("invalid pin state");
    let before = std::fs::read(&state).expect("state before");
    for (op, args) in [
        ("message_pins_list", json!({"group_id":group_id})),
        (
            "message_pins_set",
            json!({"group_id":group_id,"event_id":"same","pinned":false}),
        ),
    ] {
        let result = call(&home, op, args);
        assert_eq!(
            result.error.expect("invalid state").code,
            "pin_state_invalid"
        );
        assert_eq!(std::fs::read(&state).expect("unchanged state"), before);
    }
}

#[test]
fn message_pins_refuse_new_entries_at_the_limit() {
    let (_temp, home, groups, group_id) = fixture();
    let path = groups.ledger_path(&group_id).expect("ledger path");
    let events = (0..101)
        .map(|i| event(&group_id, &format!("message {i}")))
        .collect::<Vec<_>>();
    for event in &events {
        ledger::append(&path, event).expect("append fixture");
    }
    let pins = events[..100]
        .iter()
        .map(|event| json!({"event_id":event.id,"pinned_at":"2026-01-01T00:00:00Z"}))
        .collect::<Vec<_>>();
    let state = groups
        .group_dir(&group_id)
        .expect("group dir")
        .join("state/message_pins/pins.json");
    cccc_core::fs::write_json(&state, &json!({"v":1,"pins":pins})).expect("full pin state");
    let before = std::fs::read(&state).expect("state before");
    let result = call(
        &home,
        "message_pins_set",
        json!({"group_id":group_id,"event_id":events[100].id,"pinned":true,"detail":"ids"}),
    );
    assert_eq!(result.error.expect("limit").code, "pin_limit_reached");
    assert_eq!(std::fs::read(&state).expect("state after"), before);
    let duplicate = call(
        &home,
        "message_pins_set",
        json!({"group_id":group_id,"event_id":events[0].id,"pinned":true,"detail":"ids"}),
    );
    assert!(duplicate.ok, "{:?}", duplicate.error);
    assert_eq!(
        duplicate.result["pins"].as_array().expect("pins").len(),
        100
    );
}

#[tokio::test]
async fn message_pins_serialize_concurrent_clients() {
    let (_temp, home, groups, group_id) = fixture();
    let path = groups.ledger_path(&group_id).expect("ledger path");
    let events = (0..12)
        .map(|i| event(&group_id, &format!("concurrent {i}")))
        .collect::<Vec<_>>();
    for event in &events {
        ledger::append(&path, event).expect("append fixture");
    }
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let address = cccc_daemon::DaemonPaths::new(home.clone()).address;
    for _ in 0..200 {
        if address.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(address.exists(), "isolated daemon started");
    let client = DaemonClient::new(home.clone());
    let writes = events.iter().map(|event| {
        let client = client.clone();
        let request = request(
            "message_pins_set",
            json!({"group_id":group_id,"event_id":event.id,"pinned":true,"detail":"ids"}),
        );
        async move { client.call(&request).await.expect("daemon response") }
    });
    for response in futures_util::future::join_all(writes).await {
        assert!(response.ok, "{:?}", response.error);
    }
    let listed = client
        .call(&request(
            "message_pins_list",
            json!({"group_id":group_id,"detail":"ids"}),
        ))
        .await
        .expect("list response");
    assert!(listed.ok, "{:?}", listed.error);
    let ids = listed.result["pins"]
        .as_array()
        .expect("pins")
        .iter()
        .map(|pin| pin["event_id"].as_str().expect("id"))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), events.len());
    assert!(events.iter().all(|event| ids.contains(event.id.as_str())));

    client
        .call(&request("shutdown", json!({})))
        .await
        .expect("shutdown response");
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("daemon stopped")
        .expect("daemon task")
        .expect("daemon result");
}
