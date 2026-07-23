use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn headless_actor_uses_structured_turns_without_a_pty() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"runtime state"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"headless1","runtime":"codex","runner":"headless","by":"user"}),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"headless1","by":"user"}),
    );
    assert!(cccc_runtime::status(group_id, "headless1").is_err());
    call(
        &home,
        "headless_set_status",
        json!({"group_id":group_id,"actor_id":"headless1","status":"working","task_id":"task-1"}),
    );
    let state = call(
        &home,
        "headless_status",
        json!({"group_id":group_id,"actor_id":"headless1"}),
    );
    assert_eq!(state.result["state"]["status"], "working");
    assert_eq!(state.result["state"]["task_id"], "task-1");

    for text in ["first", "second"] {
        call(
            &home,
            "send",
            json!({"group_id":group_id,"by":"user","to":["headless1"],"text":text}),
        );
    }
    let turn = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1"}),
    );
    assert_eq!(turn.result["status"], "work_available");
    assert_eq!(
        turn.result["turn"]["messages"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(
        turn.result["turn"]["system_prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains("headless1"))
    );
    let event_ids = turn.result["turn"]["event_ids"]
        .as_array()
        .cloned()
        .expect("event ids");
    let rejected = raw_call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1","status":"done","event_ids":[event_ids[1].clone()]}),
    );
    assert!(!rejected.ok);
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("non_contiguous_turn_events")
    );

    let completed = call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1","status":"done","event_ids":event_ids}),
    );
    assert_eq!(completed.result["cursor_committed"], true);
    let idle = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"headless1","by":"headless1"}),
    );
    assert_eq!(idle.result["status"], "idle");
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn raw_call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
