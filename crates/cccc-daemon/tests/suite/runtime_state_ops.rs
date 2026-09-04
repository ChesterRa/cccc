use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

#[test]
fn web_model_actor_uses_the_structured_turn_contract_without_a_terminal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"runtime state"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","by":"user"}),
    );
    GroupStore::new(home.clone())
        .expect("group store")
        .mutate(group_id, |group| {
            group.running = true;
            Ok(())
        })
        .expect("enable structured runtime fixture");
    assert!(cccc_runtime::status(group_id, "web1").is_err());

    for text in ["first", "second"] {
        call(
            &home,
            "send",
            json!({"group_id":group_id,"by":"user","to":["web1"],"text":text,"message_mode":"send"}),
        );
    }
    let turn = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"","by":"web1"}),
    );
    assert_eq!(turn.result["status"], "work_available");
    assert_eq!(
        turn.result["turn"]["messages"].as_array().map(Vec::len),
        Some(2)
    );
    let coalesced = turn.result["turn"]["coalesced_text"]
        .as_str()
        .expect("coalesced text");
    assert!(coalesced.contains("[cccc] user → web1 [event_id="));
    assert!(coalesced.contains("]: first"));
    assert!(coalesced.contains("]: second"));
    assert!(!coalesced.contains(cccc_core::system_prompt::NEW_MESSAGE_MODE_GUIDANCE));
    assert!(
        turn.result["turn"]["system_prompt"]
            .as_str()
            .is_some_and(|prompt| {
                prompt.contains("web1")
                    && prompt.contains(cccc_core::system_prompt::NEW_MESSAGE_MODE_GUIDANCE)
                    && prompt.contains(cccc_core::system_prompt::EXISTING_MESSAGE_REPLY_GUIDANCE)
            })
    );
    let event_ids = turn.result["turn"]["event_ids"]
        .as_array()
        .cloned()
        .expect("event ids");
    for event_id in &event_ids {
        let event_id = event_id.as_str().expect("event id");
        assert!(coalesced.contains(&format!("[event_id={event_id}]")));
    }
    let turn_id = turn.result["turn"]["turn_id"]
        .as_str()
        .expect("turn id")
        .to_owned();
    let rejected = raw_call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done","turn_id":turn_id,"event_ids":[event_ids[1].clone()]}),
    );
    assert!(!rejected.ok);
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("completion_conflict")
    );
    let stale = raw_call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done","turn_id":"wrong-turn","event_ids":event_ids}),
    );
    assert!(!stale.ok);
    assert_eq!(
        stale.error.as_ref().map(|error| error.code.as_str()),
        Some("stale_turn")
    );

    let completed = call(
        &home,
        "runtime_complete_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1","status":"done","event_ids":event_ids}),
    );
    assert!(completed.result.get("cursor_committed").is_none());
    assert_eq!(completed.result["turn_id"], turn_id);
    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
    );
    assert_eq!(
        inbox.result["messages"],
        json!([]),
        "direct runtime work must not enter the Mail Inbox"
    );
    let idle = call(
        &home,
        "runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1","by":"web1"}),
    );
    assert_eq!(idle.result["status"], "idle");
}

#[test]
fn managed_terminal_actor_exposes_daemon_owned_structured_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"managed status"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"codex-1","runtime":"codex","by":"user"}),
    );

    let status = call(
        &home,
        "headless_status",
        json!({"group_id":group_id,"actor_id":"codex-1","by":"user"}),
    );
    assert_eq!(status.result["state"]["status"], "idle");

    let write = raw_call(
        &home,
        "headless_set_status",
        json!({"group_id":group_id,"actor_id":"codex-1","status":"working","by":"user"}),
    );
    assert_eq!(
        write.error.as_ref().map(|error| error.code.as_str()),
        Some("provider_managed_headless")
    );
}
#[test]
fn runtime_wait_rejects_an_unknown_explicit_transport_without_claiming_work() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"invalid transport"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"web1","runtime":"web_model",
            "by":"user"
        }),
    );
    GroupStore::new(home.clone())
        .expect("group store")
        .mutate(group_id, |group| {
            group.running = true;
            Ok(())
        })
        .expect("enable structured runtime fixture");
    call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["web1"],"text":"pending",
            "message_mode":"send"
        }),
    );

    let response = raw_call(
        &home,
        "runtime_wait_next_turn",
        json!({
            "group_id":group_id,"actor_id":"web1","by":"web1","transport":"web_model_typo"
        }),
    );
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_transport")
    );
    let ledger_path = GroupStore::new(home.clone())
        .expect("group store")
        .ledger_path(group_id)
        .expect("ledger path");
    assert!(
        cccc_core::ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .all(|event| event.kind != "runtime.delivery")
    );
}

#[cfg(unix)]
#[test]
fn terminal_actor_requires_an_attached_project_scope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"headless scope"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"codex-terminal",
            "runtime":"codex",
            "command":["sh","-c","while IFS= read -r line; do :; done"],
            "by":"user"
        }),
    );

    let started = raw_call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"codex-terminal","by":"user"}),
    );

    assert!(
        !started.ok,
        "scope-less terminal actor unexpectedly started"
    );
    assert_eq!(
        started.error.as_ref().map(|error| error.code.as_str()),
        Some("missing_project_root")
    );
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
// Included by the crate-level integration test harness.
