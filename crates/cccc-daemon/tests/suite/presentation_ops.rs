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
    assert!(published.result["event_id"].as_str().is_some());

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
