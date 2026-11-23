//! Historical bound tickets remain readable after browser issuance is retired.
use super::*;

pub(super) fn previously_issued(f: &Fixture) -> SessionNodeTicket {
    let mut ticket = create_invite(&f.home).expect("fixture endpoint and invitation ID");
    ticket.secret = "previously-issued-application-secret".into();
    ticket.relay_token = ticket
        .relay_url
        .as_ref()
        .map(|_| "previously-issued-relay-secret".into());
    ticket.expires_at = (chrono::Utc::now() + chrono::Duration::minutes(3)).to_rfc3339();
    store::update(&f.home, |state| {
        // Fixed previous-format digests, independent of the reader under test.
        let invite = state
            .invites
            .get_mut(&ticket.invite_id)
            .expect("stored fixture");
        invite.secret_hash =
            "bdd6ee6cf906e5b07220bf3cc6eebf67d67178fed3da10ebd7794e3845598865".into();
        invite.relay_secret_hash = ticket
            .relay_token
            .as_ref()
            .map(|_| "ac727c29d9e9fe0daf2be59c6625f53f413ca649831cc265cb23b6315e6547fc".into());
        invite.expected_node_id = Some("confirmed".into());
        invite.expires_at.clone_from(&ticket.expires_at);
        Ok(())
    })
    .expect("previously issued record");
    ticket
}

#[test]
fn issued_ticket_enrolls_only_its_confirmed_identity_and_keeps_existing_grants() {
    let f = Fixture::new();
    let ticket = previously_issued(&f);
    let proof = Some((ticket.invite_id.as_str(), ticket.secret.as_str()));
    assert!(pair(&f.home, "gateway", "different", "Other", proof).is_err());
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
    let legacy = create_invite(&f.home).expect("new manual ticket");
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
fn issued_relay_binding_expiry_and_revocation_still_require_a_fresh_manual_ticket() {
    let f = super::relay_tests::configured();
    let other = f.pair("other");
    let ticket = previously_issued(&f);
    let proof = Some((
        ticket.invite_id.as_str(),
        ticket.relay_token.as_deref().expect("relay capability"),
    ));
    assert!(authorize_relay(&f.home, "gateway", "different", proof).is_err());
    authorize_relay(&f.home, "gateway", "confirmed", proof).expect("previously approved identity");
    let old = pair(
        &f.home,
        "gateway",
        "confirmed",
        "Laptop",
        Some((&ticket.invite_id, &ticket.secret)),
    )
    .expect("old issued ticket");
    let pending = previously_issued(&f);
    revoke(&f.home, "confirmed").expect("explicit revocation");
    assert!(
        pair(
            &f.home,
            "gateway",
            "confirmed",
            "Laptop",
            Some((&pending.invite_id, &pending.secret))
        )
        .is_err()
    );
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "confirmed",
            Some((
                &pending.invite_id,
                pending.relay_token.as_deref().expect("relay capability")
            ))
        )
        .is_err()
    );
    let expired = previously_issued(&f);
    store::update(&f.home, |state| {
        state
            .invites
            .get_mut(&expired.invite_id)
            .expect("issued fixture")
            .expires_at = "2000-01-01T00:00:00Z".into();
        Ok(())
    })
    .expect("expired saved ticket");
    assert!(
        pair(
            &f.home,
            "gateway",
            "confirmed",
            "Laptop",
            Some((&expired.invite_id, &expired.secret))
        )
        .is_err()
    );
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "confirmed",
            Some((
                &expired.invite_id,
                expired.relay_token.as_deref().expect("relay capability")
            ))
        )
        .is_err()
    );
    let fresh = create_invite(&f.home).expect("new manual authorization");
    let peer = pair(
        &f.home,
        "gateway",
        "confirmed",
        "Laptop",
        Some((&fresh.invite_id, &fresh.secret)),
    )
    .expect("fresh grant");
    assert_ne!(peer.grant_id, old.grant_id);
    assert_eq!(
        pair(&f.home, "gateway", "other", "Other", None)
            .expect("other device unaffected")
            .grant_id,
        other.grant_id
    );
}

#[test]
fn legacy_state_rewrite_cannot_downgrade_a_previously_issued_bound_ticket() {
    let f = super::relay_tests::configured();
    let ticket = previously_issued(&f);
    let path = store::directory(&f.home).join("state.json");
    let mut old: serde_json::Value = crate::fs::read_json(&path).expect("saved fixture");
    old["invites"][&ticket.invite_id]
        .as_object_mut()
        .expect("invite")
        .remove("expected_node_id");
    old.as_object_mut().expect("state").remove("revocations");
    old["hub"]
        .as_object_mut()
        .expect("gateway")
        .remove("bound_invites");
    crate::fs::write_secret_json(&path, &old).expect("simulate old reader");
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
                ticket.relay_token.as_deref().expect("relay capability")
            ))
        )
        .is_err()
    );
    let legacy = create_invite(&f.home).expect("manual pairing remains available");
    pair(
        &f.home,
        "gateway",
        "legacy",
        "Legacy",
        Some((&legacy.invite_id, &legacy.secret)),
    )
    .expect("unbound manual ticket");
}
