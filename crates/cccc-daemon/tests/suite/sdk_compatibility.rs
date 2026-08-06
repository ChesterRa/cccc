// Included by the crate-level integration test harness.
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, json};

fn request(op: &str) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.to_owned(),
        args: Map::new(),
    }
}

#[test]
fn ping_exposes_python_compatible_sdk_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let response = cccc_daemon::handle_request(&home, &request("ping"));

    assert!(response.ok);
    assert_eq!(response.result["ipc_v"], json!(1));
    assert_eq!(response.result["implementation"], json!("rust"));
    assert_eq!(
        response.result["capabilities"]["events_stream"],
        json!(true)
    );
    assert_eq!(
        response.result["capabilities"]["remote_access"],
        json!(true)
    );
    assert!(response.result["pid"].as_u64().is_some());
    assert!(
        response.result["version"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let timestamp = response.result["ts"].as_str().expect("ping timestamp");
    chrono::DateTime::parse_from_rfc3339(timestamp).expect("RFC 3339 timestamp");
}

#[test]
fn sdk_operation_probes_recognize_send_and_tracked_send() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");

    for op in ["send", "tracked_send"] {
        let response = cccc_daemon::handle_request(&home, &request(op));
        assert!(!response.ok, "{op} probe unexpectedly succeeded");
        assert_ne!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("unknown_op"),
            "{op} must be discoverable through the SDK compatibility probe"
        );
    }
}
