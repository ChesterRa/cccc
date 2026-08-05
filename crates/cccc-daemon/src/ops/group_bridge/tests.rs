use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, integration_state};
use serde_json::json;
use tempfile::tempdir;

use super::{STORE_KEY, delivery_status, normalize_outbound_payload, validate_remote_payload};

#[test]
fn delivery_status_reads_python_compatible_receipt() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path()).expect("home path");
    home.initialize().expect("home");
    integration_state::global_update(&home, STORE_KEY, |state| {
        *state = json!({
            "registrations":[{"registration_id":"greg_1","group_id":"g_local","status":"active"}],
            "deliveries":[{"registration_id":"greg_1","idempotency_key":"once","status":"delivered"}]
        });
        Ok(())
    })
    .expect("state");
    let result = delivery_status(
        &home,
        &DaemonRequest {
            v: 1,
            op: "remote_delivery_status".into(),
            args: json!({
                "group_id":"g_local","registration_id":"greg_1","idempotency_key":"once"
            })
            .as_object()
            .cloned()
            .expect("args"),
        },
    )
    .expect("status");
    assert_eq!(result["receipt"]["status"], "delivered");
}

#[test]
fn outbound_peer_message_requires_insight_before_side_effects() {
    let request = DaemonRequest {
        v: 1,
        op: "remote_send".into(),
        args: json!({
            "by":"peer-a","require_peer_insight":true,
            "payload":{"text":"review this","to":["@foreman"]}
        })
        .as_object()
        .cloned()
        .expect("args"),
    };
    let mut payload = request.args["payload"]
        .as_object()
        .cloned()
        .expect("payload");
    let error = normalize_outbound_payload(&request, &mut payload).expect_err("missing insight");
    assert_eq!(error.code, "peer_insight_required");
    assert_eq!(error.details["new_side_effects"], false);
}

#[test]
fn remote_payload_rejects_refs_and_normalizes_recipients() {
    let mut payload = json!({
        "text":"hello","to":[" @foreman ",7],"refs":[{"event_id":"e1"}]
    })
    .as_object()
    .cloned()
    .expect("payload");
    let error = validate_remote_payload(&mut payload).expect_err("unsupported refs");
    assert_eq!(error.code, "unsupported_refs");
}
