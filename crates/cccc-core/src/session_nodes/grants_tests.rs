use super::*;

#[test]
fn revoked_enrollments_release_slots_without_reusing_consumed_tickets() {
    let f = Fixture::new();
    let mut tickets = Vec::new();
    for index in 0..32 {
        let node = format!("retired-{index}");
        let ticket = create_invite(&f.home).expect("ticket");
        pair(
            &f.home,
            "gateway",
            &node,
            &node,
            Some((&ticket.invite_id, &ticket.secret)),
        )
        .expect("enroll");
        revoke(&f.home, &node).expect("revoke");
        tickets.push((node, ticket));
    }
    assert_eq!(
        peers(&f.home)
            .expect("peers")
            .iter()
            .filter(|peer| peer.enabled)
            .count(),
        0
    );
    f.pair("new-device");
    assert_eq!(
        peers(&f.home)
            .expect("peers")
            .iter()
            .filter(|peer| peer.enabled)
            .count(),
        1
    );
    for (node, ticket) in tickets {
        assert!(pair(&f.home, "gateway", &node, &node, None).is_err());
        assert!(
            pair(
                &f.home,
                "gateway",
                &node,
                &node,
                Some((&ticket.invite_id, &ticket.secret))
            )
            .is_err()
        );
        assert!(set_connected(&f.home, "gateway", &node, true).is_err());
    }
}

#[test]
fn activity_limit_counts_disconnected_grants_and_reauthorization_but_not_history() {
    let f = Fixture::new();
    f.pair("revoked");
    revoke(&f.home, "revoked").expect("revoke");
    for index in 0..32 {
        let node = format!("active-{index}");
        f.pair(&node);
        set_connected(&f.home, "gateway", &node, false).expect("disconnect");
    }
    let ticket = create_invite(&f.home).expect("ticket");
    for node in ["new-device", "revoked"] {
        let error = pair(
            &f.home,
            "gateway",
            node,
            node,
            Some((&ticket.invite_id, &ticket.secret)),
        )
        .expect_err("32 enabled grants remain the hard limit");
        assert!(error.to_string().contains("device limit"), "{error}");
    }
    // Resuming an existing grant is not admission and still works at capacity.
    pair(&f.home, "gateway", "active-0", "active-0", None).expect("resume");
    revoke(&f.home, "active-0").expect("release slot");
    pair(
        &f.home,
        "gateway",
        "revoked",
        "revoked",
        Some((&ticket.invite_id, &ticket.secret)),
    )
    .expect("failed quota checks must not consume a valid ticket");
    assert_eq!(
        peers(&f.home)
            .expect("peers")
            .iter()
            .filter(|peer| peer.enabled)
            .count(),
        32
    );
}

#[test]
fn bounded_history_eviction_cannot_restore_a_revoked_grant_or_source() {
    let f = Fixture::new();
    let ticket = create_invite(&f.home).expect("ticket");
    let first = pair(
        &f.home,
        "gateway",
        "oldest",
        "Oldest",
        Some((&ticket.invite_id, &ticket.secret)),
    )
    .expect("first grant");
    let old_source = f.sources("oldest").remove(0);
    revoke(&f.home, "oldest").expect("revoke");
    for index in 0..40 {
        let node = format!("retired-{index}");
        f.pair(&node);
        f.sources(&node);
        revoke(&f.home, &node).expect("revoke");
    }
    let state = store::read(&f.home).expect("persisted history");
    assert_eq!(state.peers.len(), 32);
    assert_eq!(state.peers.values().filter(|peer| peer.enabled).count(), 0);
    assert!(!state.peers.contains_key("oldest"));
    assert_eq!(state.sources.len(), 32);
    assert!(!state.sources.contains_key(&old_source.source.source_id));
    for node in ["oldest", "retired-39"] {
        assert!(pair(&f.home, "gateway", node, node, None).is_err());
        assert!(
            pair(
                &f.home,
                "gateway",
                node,
                node,
                Some((&ticket.invite_id, &ticket.secret))
            )
            .is_err()
        );
        assert!(set_connected(&f.home, "gateway", node, true).is_err());
    }
    let replacement = f.pair("oldest");
    assert_ne!(replacement.grant_id, first.grant_id);
    assert!(source(&f.home, &old_source.source.source_id).is_err());
    let new_source = f.sources("oldest").remove(0);
    assert_ne!(new_source.source.source_id, old_source.source.source_id);
    assert!(source(&f.home, &new_source.source.source_id).is_ok());
    assert_eq!(
        pair(&f.home, "gateway", "oldest", "Oldest", None)
            .expect("resume new grant")
            .grant_id,
        replacement.grant_id
    );
}

#[test]
fn manual_ticket_issue_preserves_previous_keys_grants_and_optional_state_fields() {
    let f = Fixture::new();
    let granted = f.pair("confirmed");
    let key = identity(&f.home, "client", || "historical-test-key".into()).expect("saved key");
    let issued = super::join_tests::previously_issued(&f);
    store::update(&f.home, |state| {
        let record = serde_json::json!({"generation":"00000000-0000-4000-8000-000000000000","expires_at":issued.expires_at});
        state.revocations.insert("confirmed".into(), serde_json::from_value(record).expect("previous metadata"));
        state.hub.as_mut().expect("old gateway metadata").bound_invites = true;
        Ok(())
    }).expect("historical optional fields");
    let path = store::directory(&f.home).join("state.json");
    let before: serde_json::Value = crate::fs::read_json(&path).expect("stored state");
    create_invite(&f.home).expect("new manual ticket");
    let after: serde_json::Value = crate::fs::read_json(&path).expect("round-trip state");
    assert_eq!(before["revocations"], after["revocations"]);
    assert_eq!(
        before["invites"][&issued.invite_id],
        after["invites"][&issued.invite_id]
    );
    assert_eq!(before["hub"], after["hub"]);
    assert!(identity(&f.home, "client", || "must-not-replace".into()).expect("same key") == key);
    assert_eq!(
        pair(&f.home, "gateway", "confirmed", "Laptop", None)
            .expect("resume saved grant")
            .grant_id,
        granted.grant_id
    );
}
