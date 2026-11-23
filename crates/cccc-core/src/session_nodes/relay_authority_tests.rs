use super::*;

#[test]
fn relay_pairing_binds_one_key_then_resumes_and_revokes_with_the_existing_device_grant() {
    let f = configured();
    let invite = create_invite(&f.home).expect("capability");
    let before =
        std::fs::read(store::directory(&f.home).join("state.json")).expect("private state");
    assert_ne!(invite.relay_token.as_deref(), Some(invite.secret.as_str()));
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "node-a",
            Some((&invite.invite_id, &invite.secret))
        )
        .is_err()
    );
    assert!(
        pair(
            &f.home,
            "gateway",
            "node-a",
            "Laptop",
            Some((
                &invite.invite_id,
                invite.relay_token.as_deref().expect("relay capability")
            ))
        )
        .is_err(),
        "Relay permission is not device enrollment authority"
    );
    assert!(authorize_relay(&f.home, "gateway", "node-a", None).is_err());
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "node-a",
            Some((&invite.invite_id, "wrong"))
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(store::directory(&f.home).join("state.json")).expect("state"),
        before
    );
    authorize_relay(
        &f.home,
        "gateway",
        "node-a",
        Some((
            &invite.invite_id,
            invite.relay_token.as_deref().expect("relay capability"),
        )),
    )
    .expect("authenticated pending key");
    assert!(
        relay_authorities(&f.home, "gateway")
            .expect("pending authority")
            .contains("node-a")
    );
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "node-b",
            Some((
                &invite.invite_id,
                invite.relay_token.as_deref().expect("relay capability")
            ))
        )
        .is_err()
    );
    assert!(pair(&f.home, "gateway", "node-b", "Other", None).is_err());
    let peer = pair(
        &f.home,
        "gateway",
        "node-a",
        "Laptop",
        Some((&invite.invite_id, &invite.secret)),
    )
    .expect("enrollment");
    authorize_relay(&f.home, "gateway", "node-a", None).expect("resume by signed device identity");
    let fresh = create_invite(&f.home).expect("route refresh");
    assert_eq!(
        pair(
            &f.home,
            "gateway",
            "node-a",
            "Laptop",
            Some((&fresh.invite_id, &fresh.secret))
        )
        .expect("refresh existing device")
        .grant_id,
        peer.grant_id
    );
    assert!(
        pair(
            &f.home,
            "gateway",
            "node-b",
            "Other",
            Some((&fresh.invite_id, &fresh.secret))
        )
        .is_err(),
        "A route refresh still consumes the ticket"
    );
    revoke(&f.home, "node-a").expect("revoke");
    assert!(
        !relay_authorities(&f.home, "gateway")
            .expect("authority")
            .contains("node-a")
    );
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "node-a",
            Some((
                &invite.invite_id,
                invite.relay_token.as_deref().expect("relay capability")
            ))
        )
        .is_err()
    );
    assert!(pair(&f.home, "gateway", "node-a", "Laptop", None).is_err());
    let independent = create_invite(&f.home).expect("independent enrollment");
    authorize_relay(
        &f.home,
        "gateway",
        "network-only",
        Some((
            &independent.invite_id,
            independent
                .relay_token
                .as_deref()
                .expect("relay capability"),
        )),
    )
    .expect("bootstrap relay use");
    pair(
        &f.home,
        "gateway",
        "direct-owner",
        "Direct",
        Some((&independent.invite_id, &independent.secret)),
    )
    .expect("A relay-only capability cannot block the owner of the end-to-end pairing secret");
    assert!(
        !relay_authorities(&f.home, "gateway")
            .expect("retired bootstrap")
            .contains("network-only")
    );
    authorize_relay(&f.home, "gateway", "direct-owner", None).expect("actual enrolled identity");
    let pending = create_invite(&f.home).expect("new pending key");
    authorize_relay(
        &f.home,
        "gateway",
        "pending",
        Some((
            &pending.invite_id,
            pending.relay_token.as_deref().expect("relay capability"),
        )),
    )
    .expect("pending relay");
    store::update(&f.home, |state| {
        state
            .invites
            .get_mut(&pending.invite_id)
            .expect("pending")
            .expires_at = "2000-01-01T00:00:00Z".into();
        Ok(())
    })
    .expect("expire without sleeping");
    assert!(
        !relay_authorities(&f.home, "gateway")
            .expect("expired authority")
            .contains("pending")
    );
    assert!(
        authorize_relay(
            &f.home,
            "gateway",
            "pending",
            Some((
                &pending.invite_id,
                pending.relay_token.as_deref().expect("relay capability")
            ))
        )
        .is_err()
    );
}
