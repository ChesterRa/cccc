use super::*;

#[test]
fn approved_ticket_enrolls_only_the_confirmed_identity_and_keeps_legacy_tickets() {
    let f = Fixture::new();
    let deadline = (chrono::Utc::now() + chrono::Duration::minutes(3)).to_rfc3339();
    let ticket = create_bound_invite(
        &f.home,
        "confirmed",
        &deadline,
        &capture_join_generation(&f.home, "confirmed", &deadline).expect("generation"),
    )
    .expect("approved ticket");
    let proof = Some((ticket.invite_id.as_str(), ticket.secret.as_str()));
    assert!(pair(&f.home, "gateway", "different", "Other", proof).is_err());
    assert_eq!(ticket.expires_at, deadline);
    let first = pair(&f.home, "gateway", "confirmed", "Laptop", proof).expect("native identity");
    assert!(
        memberships(&f.home, "confirmed")
            .expect("no Group rights")
            .is_empty()
    );
    assert_eq!(
        pair(&f.home, "gateway", "confirmed", "Laptop", proof)
            .expect("lost reply retry")
            .grant_id,
        first.grant_id
    );
    let legacy = create_invite(&f.home).expect("legacy ticket");
    assert!(
        pair(
            &f.home,
            "gateway",
            "different",
            "Other",
            Some((&legacy.invite_id, &legacy.secret))
        )
        .is_ok()
    );
}

#[test]
fn approval_binds_relay_before_first_use_and_revocation_cancels_unconsumed_tickets() {
    let f = super::relay_tests::configured();
    f.pair("confirmed");
    let deadline = (chrono::Utc::now() + chrono::Duration::minutes(3)).to_rfc3339();
    let ticket = create_bound_invite(
        &f.home,
        "confirmed",
        &deadline,
        &capture_join_generation(&f.home, "confirmed", &deadline).expect("generation"),
    )
    .expect("approved ticket");
    let relay_proof = Some((
        ticket.invite_id.as_str(),
        ticket.relay_token.as_deref().expect("relay token"),
    ));
    assert!(authorize_relay(&f.home, "gateway", "different", relay_proof).is_err());
    revoke(&f.home, "confirmed").expect("revoke device");
    assert!(
        pair(
            &f.home,
            "gateway",
            "confirmed",
            "Laptop",
            Some((&ticket.invite_id, &ticket.secret))
        )
        .is_err()
    );
    assert!(authorize_relay(&f.home, "gateway", "confirmed", relay_proof).is_err());
    let fresh = create_bound_invite(
        &f.home,
        "confirmed",
        &deadline,
        &capture_join_generation(&f.home, "confirmed", &deadline).expect("new generation"),
    )
    .expect("new explicit approval");
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "confirmed",
            Some((
                &fresh.invite_id,
                fresh.relay_token.as_deref().expect("relay token")
            ))
        )
        .is_ok()
    );
    pair(
        &f.home,
        "gateway",
        "confirmed",
        "Laptop",
        Some((&fresh.invite_id, &fresh.secret)),
    )
    .expect("fresh grant");
}

#[test]
fn cross_process_revocation_invalidates_pending_approval_without_affecting_other_devices() {
    let f = Fixture::new();
    f.pair("a");
    f.pair("b");
    let deadline = (chrono::Utc::now() + chrono::Duration::minutes(3)).to_rfc3339();
    let a = capture_join_generation(&f.home, "a", &deadline).expect("A request baseline");
    let b = capture_join_generation(&f.home, "b", &deadline).expect("B request baseline");
    revoke(&f.home, "a").expect("same core entry used by another CLI process");
    assert!(create_bound_invite(&f.home, "a", &deadline, &a).is_err());
    assert!(create_bound_invite(&f.home, "b", &deadline, &b).is_ok());
    for index in 0..33 {
        let node = format!("history-{index}");
        f.pair(&node);
        revoke(&f.home, &node).expect("history churn");
    }
    assert!(
        peers(&f.home)
            .expect("bounded history")
            .iter()
            .all(|p| p.node_id != "a")
    );
    assert!(create_bound_invite(&f.home, "a", &deadline, &a).is_err());
    let new = capture_join_generation(&f.home, "a", &deadline).expect("new request after revoke");
    let fresh = create_bound_invite(&f.home, "a", &deadline, &new).expect("new explicit approval");
    pair(
        &f.home,
        "gateway",
        "a",
        "A",
        Some((&fresh.invite_id, &fresh.secret)),
    )
    .expect("new grant");
    assert!(create_bound_invite(&f.home, "a", &deadline, &a).is_err());
}

#[test]
fn legacy_state_rewrite_cannot_downgrade_bound_tickets_or_revocation_baselines() {
    let f = super::relay_tests::configured();
    let deadline = (chrono::Utc::now() + chrono::Duration::minutes(3)).to_rfc3339();
    let generation =
        capture_join_generation(&f.home, "confirmed", &deadline).expect("first baseline");
    assert!(!generation.is_empty());
    assert!(
        !join_generation_matches(&f.home, "other", "").expect("empty baseline is never authority")
    );
    let ticket = create_bound_invite(&f.home, "confirmed", &deadline, &generation)
        .expect("bound invitation");
    let path = store::directory(&f.home).join("state.json");
    let mut old: serde_json::Value = crate::fs::read_json(&path).expect("state fixture");
    old["invites"][&ticket.invite_id]
        .as_object_mut()
        .expect("invite")
        .remove("expected_node_id");
    old.as_object_mut().expect("state").remove("revocations");
    old["hub"]
        .as_object_mut()
        .expect("gateway")
        .remove("bound_invites");
    crate::fs::write_secret_json(&path, &old)
        .expect("simulate old reader retaining only known fields");
    assert!(
        !join_generation_matches(&f.home, "confirmed", &generation)
            .expect("no missing baseline match")
    );
    assert!(!bound_invites_supported(&f.home).expect("legacy gateway"));
    assert!(
        pair(
            &f.home,
            "gateway",
            "confirmed",
            "Laptop",
            Some((&ticket.invite_id, &ticket.secret))
        )
        .is_err()
    );
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "confirmed",
            Some((
                &ticket.invite_id,
                ticket.relay_token.as_deref().expect("relay secret")
            ))
        )
        .is_err()
    );
    let new_generation =
        capture_join_generation(&f.home, "confirmed", &deadline).expect("fresh baseline");
    assert!(create_bound_invite(&f.home, "confirmed", &deadline, &new_generation).is_err());
    let legacy = create_invite(&f.home).expect("legacy manual pairing still works");
    pair(
        &f.home,
        "gateway",
        "legacy",
        "Legacy",
        Some((&legacy.invite_id, &legacy.secret)),
    )
    .expect("old unbound ticket");
}
