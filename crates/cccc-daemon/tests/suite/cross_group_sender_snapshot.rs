use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn local_cross_group_relay_preserves_the_source_actor_display_name() {
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
            "actor_id":"claude-1",
            "title":"项目总监",
            "runtime":"claude",
            "by":"user"
        }),
    );

    let request = json!({
        "group_id":source_group_id,
        "dst_group_id":destination_group_id,
        "by":"claude-1",
        "to":["user"],
        "text":"hello",
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

fn ok(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
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
