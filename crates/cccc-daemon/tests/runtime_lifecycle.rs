#![cfg(unix)]

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn actor_lifecycle_controls_terminal_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(
        &home,
        "group_create",
        json!({"title":"runtime-test","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    assert!(
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":"peer1",
                "runner":"headless",
                "runtime":"custom",
                "command":["sh","-c","printf daemon-runtime-ready; sleep 5"],
                "by":"user"
            }),
        )
        .ok
    );
    assert!(
        call(
            &home,
            "actor_start",
            json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
        )
        .ok
    );
    let groups = call(&home, "group_list", json!({}));
    let summary = groups.result["groups"]
        .as_array()
        .and_then(|groups| groups.iter().find(|group| group["group_id"] == group_id))
        .expect("group summary");
    assert_eq!(summary["running"], true);
    assert_eq!(summary["runtime_status"]["running_actor_count"], 1);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let tail = call(
        &home,
        "terminal_tail",
        json!({"group_id":group_id,"actor_id":"peer1"}),
    );
    assert!(
        tail.result["text"]
            .as_str()
            .unwrap_or_default()
            .contains("daemon-runtime-ready")
    );
    let end_cursor = tail.result["end_cursor"].as_u64().expect("end cursor");
    let since = call(
        &home,
        "terminal_since",
        json!({"group_id":group_id,"actor_id":"peer1","after":end_cursor}),
    );
    assert_eq!(since.result["history"]["data"], "");
    assert_eq!(since.result["history"]["end_cursor"], end_cursor);
    assert!(
        call(
            &home,
            "actor_stop",
            json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
        )
        .ok
    );
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"user"}),
    );
    assert_eq!(actors.result["actors"][0]["running"], false);

    assert!(
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":"peer-remove",
                "runner":"headless",
                "runtime":"custom",
                "command":["sh","-c","sleep 30"],
                "by":"user"
            }),
        )
        .ok
    );
    assert!(
        call(
            &home,
            "actor_start",
            json!({"group_id":group_id,"actor_id":"peer-remove","by":"user"}),
        )
        .ok
    );
    assert!(cccc_runtime::status(&group_id, "peer-remove").is_ok());
    assert!(
        call(
            &home,
            "actor_remove",
            json!({"group_id":group_id,"actor_id":"peer-remove","by":"user"}),
        )
        .ok
    );
    assert!(cccc_runtime::status(&group_id, "peer-remove").is_err());
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    };
    let response = cccc_daemon::handle_request(home, &request);
    assert!(
        response.ok,
        "{op} failed: {:?}",
        response.error.as_ref().map(|error| &error.message)
    );
    response
}
