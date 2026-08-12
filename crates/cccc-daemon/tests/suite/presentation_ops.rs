// Included by the crate-level integration test harness.
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn presentation_operations_match_frontend_contract_and_emit_events() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"presentation","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let published = call(
        &home,
        "presentation_publish",
        json!({
            "group_id":group_id,
            "slot":"slot-3",
            "card_type":"web_preview",
            "url":"https://example.com/dashboard",
            "by":"user"
        }),
    );
    assert_eq!(published.result["slot_id"], "slot-3");
    assert_eq!(published.result["card"]["slot_id"], "slot-3");
    assert_eq!(published.result["card"]["published_by"], "user");
    assert_eq!(published.result["presentation"]["slots"][2]["index"], 3);
    assert_eq!(published.result["replaced"], false);
    assert_eq!(published.result["event"]["kind"], "presentation.publish");
    assert_eq!(published.result["event"]["data"]["summary"], "");
    assert_eq!(
        published.result["event_id"],
        published.result["event"]["id"]
    );

    let fetched = call(&home, "presentation_get", json!({"group_id":group_id}));
    assert_eq!(
        fetched.result["presentation"]["highlight_slot_id"],
        "slot-3"
    );
    let cleared = call(
        &home,
        "presentation_clear",
        json!({"group_id":group_id,"slot":"slot-3","by":"user"}),
    );
    assert_eq!(cleared.result["cleared_slots"], json!(["slot-3"]));
    assert_eq!(cleared.result["slot_id"], "slot-3");
    assert_eq!(cleared.result["event"]["kind"], "presentation.clear");
    assert_eq!(cleared.result["event"]["data"]["cleared_all"], false);
    assert_eq!(cleared.result["event_id"], cleared.result["event"]["id"]);
    let tail = call(
        &home,
        "ledger_tail",
        json!({"group_id":group_id,"limit":10}),
    );
    assert!(
        tail.result["events"]
            .as_array()
            .expect("events")
            .iter()
            .any(|event| event["kind"] == "presentation.publish")
    );
    assert!(
        tail.result["events"]
            .as_array()
            .expect("events")
            .iter()
            .any(|event| event["kind"] == "presentation.clear")
    );
}

#[test]
fn presentation_clear_requires_a_valid_publisher_and_honors_all() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"presentation clear","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for slot in ["slot-1", "slot-2"] {
        call(
            &home,
            "presentation_publish",
            json!({"group_id":group_id,"slot":slot,"content":slot,"by":"user"}),
        );
    }

    let unauthorized = raw_call(
        &home,
        "presentation_clear",
        json!({"group_id":group_id,"slot":"slot-1","by":"ghost"}),
    );
    assert!(!unauthorized.ok);

    let cleared = call(
        &home,
        "presentation_clear",
        json!({"group_id":group_id,"slot":"slot-1","all":true,"by":"user"}),
    );
    assert_eq!(cleared.result["cleared_slots"], json!(["slot-1", "slot-2"]));
}

#[test]
fn failed_presentation_event_append_rolls_back_the_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"presentation rollback","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let ledger_path = store
        .group_dir(group_id)
        .expect("group directory")
        .join("ledger.jsonl");
    std::fs::remove_file(&ledger_path).expect("remove ledger");
    std::fs::create_dir(&ledger_path).expect("block ledger append");

    let failed = raw_call(
        &home,
        "presentation_publish",
        json!({"group_id":group_id,"slot":"slot-1","content":"must roll back","by":"user"}),
    );
    assert!(!failed.ok);
    assert!(
        cccc_core::presentation::load(&store, group_id)
            .expect("presentation")
            .slots
            .iter()
            .all(|slot| slot.card.is_none())
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
