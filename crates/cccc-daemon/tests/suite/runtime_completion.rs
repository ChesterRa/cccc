use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

#[test]
fn exact_completion_is_replayable_but_mismatched_receipts_are_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = setup(&home);
    let turn = next_turn(&home, &group_id);
    let args = completion_args(&group_id, &turn, "delivery-a");

    let first = call(&home, "web_model_runtime_complete_turn", args.clone());
    let restarted = HomeLayout::from_path(home.root().to_owned()).expect("restart home");
    let replay = call(&restarted, "web_model_runtime_complete_turn", args.clone());
    assert_eq!(replay.result, first.result);
    assert_eq!(replay.result["delivery_id"], "delivery-a");

    for mismatch in [
        json!({"delivery_id":"delivery-b"}),
        json!({"event_ids":["different-event"]}),
        json!({"status":"partial"}),
    ] {
        let mut changed = args.clone();
        changed.extend(mismatch.as_object().cloned().expect("object"));
        let rejected = raw_call(&home, "web_model_runtime_complete_turn", changed);
        assert!(!rejected.ok);
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some("completion_conflict")
        );
    }
}

#[cfg(unix)]
#[test]
fn precommit_ledger_failure_does_not_create_a_completion_receipt() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = setup(&home);
    let turn = next_turn(&home, &group_id);
    let args = completion_args(&group_id, &turn, "delivery-failure");
    let ledger = GroupStore::new(home.clone())
        .expect("store")
        .ledger_path(&group_id)
        .expect("ledger");
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o444)).expect("lock ledger");
    let failed = raw_call(&home, "web_model_runtime_complete_turn", args.clone());
    std::fs::set_permissions(&ledger, std::fs::Permissions::from_mode(0o644))
        .expect("unlock ledger");
    assert!(!failed.ok);

    let completed = call(&home, "web_model_runtime_complete_turn", args);
    assert_eq!(completed.result["delivery_id"], "delivery-failure");
}

pub(crate) fn setup(home: &HomeLayout) -> String {
    let created = call(home, "group_create", json!({"title":"completion"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"web1","runtime":"web_model","runner":"headless","by":"user"}),
    );
    call(
        home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    call(
        home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["web1"],"text":"work"}),
    );
    group_id
}

pub(crate) fn next_turn(home: &HomeLayout, group_id: &str) -> Value {
    call(
        home,
        "web_model_runtime_wait_next_turn",
        json!({"group_id":group_id,"actor_id":"web1"}),
    )
    .result["turn"]
        .clone()
}

pub(crate) fn completion_args(
    group_id: &str,
    turn: &Value,
    delivery_id: &str,
) -> Map<String, Value> {
    json!({
        "group_id":group_id,
        "actor_id":"web1",
        "by":"web1",
        "turn_id":turn["turn_id"],
        "event_ids":turn["event_ids"],
        "status":"done",
        "delivery_id":delivery_id
    })
    .as_object()
    .cloned()
    .expect("args")
}

pub(crate) fn call(home: &HomeLayout, op: &str, args: impl Into<Value>) -> DaemonResponse {
    let response = raw_call(
        home,
        op,
        args.into().as_object().cloned().unwrap_or_else(Map::new),
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

pub(crate) fn raw_call(home: &HomeLayout, op: &str, args: Map<String, Value>) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        },
    )
}
