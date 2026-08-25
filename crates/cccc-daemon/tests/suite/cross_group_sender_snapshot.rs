use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};

#[test]
fn local_peer_cross_group_relay_preserves_the_source_actor_display_name() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = ok(&home, "group_create", json!({"title":"Source Team"}));
    let destination = ok(&home, "group_create", json!({"title":"Destination Team"}));
    let source_group_id = source.result["group"]["group_id"]
        .as_str()
        .expect("source group id");
    let destination_group_id = destination.result["group"]["group_id"]
        .as_str()
        .expect("destination group id");
    ok(
        &home,
        "actor_add",
        json!({
            "group_id":source_group_id,
            "actor_id":"lead",
            "runtime":"codex",
            "by":"user"
        }),
    );
    ok(
        &home,
        "actor_add",
        json!({
            "group_id":source_group_id,
            "actor_id":"claude-1",
            "title":"项目总监",
            "runtime":"claude",
            "by":"user"
        }),
    );
    ok(
        &home,
        "actor_add",
        json!({
            "group_id":destination_group_id,
            "actor_id":"destination-lead",
            "runtime":"codex",
            "by":"user"
        }),
    );

    let request = json!({
        "group_id":source_group_id,
        "dst_group_id":destination_group_id,
        "by":" claude-1 ",
        "to":["@foreman"],
        "text":"hello",
        "message_mode":"request_reply",
        "insight":"The destination foreman owns the requested information.",
        "client_id":"cross-group-display-metadata"
    });
    let relayed = ok(&home, "send_cross_group", request.clone());

    assert_eq!(
        relayed.result["dst_event"]["by"],
        format!("{source_group_id}::claude-1")
    );
    assert_eq!(
        relayed.result["dst_event"]["data"]["sender_title"],
        "项目总监"
    );
    assert_eq!(
        relayed.result["dst_event"]["data"]["sender_runtime"],
        "claude"
    );
    assert_eq!(
        relayed.result["dst_event"]["data"]["to"],
        json!(["destination-lead"])
    );
    assert_eq!(
        relayed.result["dst_event"]["data"]["message_mode"],
        "request_reply"
    );
    assert_eq!(
        relayed.result["src_event"]["data"]["dst_group_id"],
        destination_group_id
    );
    assert!(
        relayed.result["src_event"]["data"]
            .get("to_group_id")
            .is_none()
    );
    assert!(
        relayed.result["dst_event"]["data"]
            .get("dst_group_id")
            .is_none()
    );

    let replay = ok(&home, "send_cross_group", request);
    assert_eq!(replay.result["duplicate"], true);
    assert_eq!(replay.result["src_event"], relayed.result["src_event"]);
    assert_eq!(replay.result["dst_event"], relayed.result["dst_event"]);
}

#[test]
fn local_cross_group_relay_rejects_an_unknown_source_actor_before_writing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = ok(&home, "group_create", json!({"title":"Source Team"}));
    let destination = ok(&home, "group_create", json!({"title":"Destination Team"}));
    let source_group_id = source.result["group"]["group_id"]
        .as_str()
        .expect("source group id");
    let destination_group_id = destination.result["group"]["group_id"]
        .as_str()
        .expect("destination group id");
    let store = GroupStore::new(home.clone()).expect("store");
    let source_ledger = store.ledger_path(source_group_id).expect("source ledger");
    let destination_ledger = store
        .ledger_path(destination_group_id)
        .expect("destination ledger");
    let source_events = ledger::read_all(&source_ledger)
        .expect("source events")
        .len();
    let destination_events = ledger::read_all(&destination_ledger)
        .expect("destination events")
        .len();

    let response = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source_group_id,
            "dst_group_id":destination_group_id,
            "by":"forged-actor",
            "to":["user"],
            "text":"forged message",
            "message_mode":"send"
        }),
    );

    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("permission_denied")
    );
    assert_eq!(
        response.error.as_ref().map(|error| error.message.as_str()),
        Some("unknown actor: forged-actor")
    );
    assert_eq!(
        ledger::read_all(&source_ledger)
            .expect("source events")
            .len(),
        source_events
    );
    assert_eq!(
        ledger::read_all(&destination_ledger)
            .expect("destination events")
            .len(),
        destination_events
    );
}

fn ok(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
