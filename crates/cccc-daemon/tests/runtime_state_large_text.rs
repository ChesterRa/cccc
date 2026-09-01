use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn structured_turn_preserves_inline_text_above_the_legacy_24k_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"large structured turn"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"headless1","runtime":"custom","runner":"headless","by":"user"}),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"headless1","by":"user"}),
    );
    let text = format!("large-start-{}-large-end", "x".repeat(25_000));
    call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["headless1"],"text":text,"message_mode":"send"}),
    );

    let turn = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1"}),
    );
    let coalesced = turn.result["turn"]["coalesced_text"]
        .as_str()
        .expect("coalesced text");

    assert!(coalesced.contains("large-start-"));
    assert!(coalesced.contains("-large-end"));
    assert!(coalesced.contains("To reply, use cccc_message_reply"));
    assert!(!coalesced.contains("coalesced turn text truncated"));
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
