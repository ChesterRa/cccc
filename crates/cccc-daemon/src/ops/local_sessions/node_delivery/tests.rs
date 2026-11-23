use super::*;

#[test]
fn delivery_journal_fences_unknown_results_and_rejects_identifier_payload_collisions() {
    let temp = tempfile::tempdir().expect("private Home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
    home.initialize().expect("initialize");
    assert!(
        begin(&home, "hub:grant:handoff", "payload")
            .expect("first attempt")
            .is_none()
    );
    assert!(matches!(
        begin(&home, "hub:grant:handoff", "payload").expect("unknown attempt"),
        Some(Outcome::Ambiguous { .. })
    ));
    assert!(begin(&home, "hub:grant:handoff", "different").is_err());
    finish(&home, "hub:grant:handoff", "payload", &Outcome::Accepted)
        .expect("confirmed native outcome");
    let reopened = HomeLayout::from_path(home.root()).expect("reopened Home");
    assert!(matches!(
        begin(&reopened, "hub:grant:handoff", "payload").expect("durable receipt"),
        Some(Outcome::Accepted)
    ));
    assert!(
        begin(&reopened, "hub:new-grant:handoff", "payload")
            .expect("new grant namespace")
            .is_none()
    );
    let contents = std::fs::read_to_string(home.root().join("state/session_nodes/deliveries.json"))
        .expect("private journal");
    assert!(
        !contents.contains("\"payload\""),
        "Journal retains payload fingerprints, not message bodies"
    );
}

#[test]
fn remote_attachments_fail_before_native_handoff_without_changing_local_delivery() {
    let mut actor = cccc_contracts::Actor::new("native");
    let mut event = cccc_contracts::Event::new("chat.message", "group");
    event
        .data
        .insert("attachments".into(), json!([{"path":"state/blobs/file"}]));
    actor.external_session_id = "claude:local".into();
    assert!(preflight_event(&actor, &event).is_ok());
    actor.external_session_id = "remote:node:grant:source".into();
    assert!(matches!(
        preflight_event(&actor, &event),
        Err(super::super::PreflightError::Rejected(_))
    ));
    event.data.clear();
    assert!(preflight_event(&actor, &event).is_ok());
}
