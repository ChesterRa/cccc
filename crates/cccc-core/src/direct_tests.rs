use super::*;
use cccc_contracts::connect::ConnectPeerOperation;
#[test]
fn direct_setup_and_invitation_reject_changed_listener_without_replacing_it() {
    let (_temp, a, _b, ga, _gb, _) = pair();
    let initial = load(&a).expect("load receiving store").listener;
    let replacement = DirectListener {
        bind: "[::]:9944".into(),
        address: "[fd00::9]:9944".into(),
    };
    configure_checked(&a, Some(replacement.clone()), None, Some(&initial))
        .expect("replace listener");
    let snapshot = load(&a).expect("load receiving store");
    assert!(configure_checked(&a, initial.clone(), Some("Stale"), Some(&initial)).is_err());
    assert!(invite_checked(&a, &ga, initial.as_ref()).is_err());
    assert!(load(&a).expect("load receiving store") == snapshot);
    let invitation =
        invite_checked(&a, &ga, Some(&replacement)).expect("invite with current address");
    let raw = URL_SAFE_NO_PAD
        .decode(
            invitation
                .strip_prefix("cccc-direct:")
                .expect("invitation prefix"),
        )
        .expect("valid invitation fixture");
    let invitation: DirectInvitation =
        serde_json::from_slice(&raw).expect("valid invitation fixture");
    assert_eq!(invitation.address, replacement.address);
}
fn pair() -> (
    tempfile::TempDir,
    HomeLayout,
    HomeLayout,
    String,
    String,
    String,
) {
    let temp = tempfile::tempdir().expect("temp");
    let a = HomeLayout::from_path(temp.path().join("a")).expect("home");
    let b = HomeLayout::from_path(temp.path().join("b")).expect("home");
    let ga = GroupStore::new(a.clone())
        .expect("store")
        .create("A", "")
        .expect("group")
        .group_id;
    let gb = GroupStore::new(b.clone())
        .expect("store")
        .create("B", "")
        .expect("group")
        .group_id;
    configure(
        &a,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "127.0.0.1:8847".into(),
        }),
        None,
    )
    .expect("listener");
    let invitation = invite(&a, &ga).expect("invite");
    let id = join(&b, &gb, &invitation).expect("join");
    (temp, a, b, ga, gb, id)
}
fn claim(a: &HomeLayout, b: &HomeLayout, id: &str) -> io::Result<DirectState> {
    let r = load(b)?
        .relations
        .into_iter()
        .find(|r| r.id == id)
        .expect("relation");
    hello(
        a,
        id,
        &r.invitation.expect("invite").secret,
        &r.local,
        &r.local.public_key,
    )
}
fn approve(a: &HomeLayout, b: &HomeLayout, ga: &str, id: &str) {
    assert_eq!(claim(a, b, id).expect("claim"), DirectState::Pending);
    set_state(a, ga, id, true).expect("approve");
    update(b, |s| {
        s.relations[0].state = DirectState::Active;
        Ok(())
    })
    .expect("confirm");
}
#[test]
fn grants_need_both_approvals_bind_exact_groups_and_survive_reload() {
    let (_temp, a, b, ga, gb, id) = pair();
    let peer = InstanceIdentity::load(&b).expect("key").peer_id;
    assert!(binding(&a, &peer, &id).is_err());
    assert!(set_state(&a, &ga, &id, true).is_err());
    assert!(set_state(&b, &gb, &id, true).is_err());
    approve(&a, &b, &ga, &id);
    set_state(&a, &ga, &id, true).expect("idempotent approve");
    let request = crate::connect_peer::sign_request(
        &a,
        &peer,
        ConnectPeerOperation::Catalog {
            connection_id: Some(id.clone()),
            source_group_id: ga.clone(),
            target_group_id: Some(gb.clone()),
            after: None,
        },
    )
    .expect("signed");
    assert!(crate::connect_peer::authenticate(&b, &request).is_ok());
    assert!(
        serde_json::to_value(&request).expect("wire")["proof"]
            .get("account_id")
            .is_none()
    );
    let mut forged = request.clone();
    forged.operation = ConnectPeerOperation::Catalog {
        connection_id: Some(id.clone()),
        source_group_id: ga.clone(),
        target_group_id: Some("unshared".into()),
        after: None,
    };
    assert!(crate::connect_peer::authenticate(&b, &forged).is_err());
    set_state(&a, &ga, &id, false).expect("revoke");
    assert!(crate::connect_peer::group_binding(&a, &peer, &ga, &gb).is_err());
    assert_eq!(claim(&a, &b, &id).expect("claim"), DirectState::Revoked);
    assert!(set_state(&a, &ga, &id, true).is_err());
    remove(&a, &ga, &id).expect("remove");
    assert_eq!(
        claim(&a, &b, &id).expect("retired refusal"),
        DirectState::Revoked
    );
}
#[test]
fn invitation_cannot_be_claimed_by_another_peer_or_after_expiry() {
    let (_temp, a, b, ga, _, id) = pair();
    assert_eq!(claim(&a, &b, &id).expect("claim"), DirectState::Pending);
    let r = load(&b).expect("store").relations.remove(0);
    let mut other = r.local.clone();
    other.group_id = "another".into();
    assert!(
        hello(
            &a,
            &id,
            &r.invitation.expect("invite").secret,
            &other,
            &other.public_key
        )
        .is_err()
    );
    update(&a, |s| {
        s.relations[0].expires_at = "2000-01-01T00:00:00Z".into();
        Ok(())
    })
    .expect("expire");
    assert!(set_state(&a, &ga, &id, true).is_err());
    assert_eq!(
        claim(&a, &b, &id).expect("expired refusal"),
        DirectState::Expired
    );
    assert!(set_state(&a, &ga, &id, true).is_err());
}
#[test]
fn pending_joiner_needs_a_receiver_decision_or_explicit_cancellation_before_removal() {
    let (_temp, _a, b, _ga, gb, id) = pair();
    update(&b, |s| {
        s.relations[0].expires_at = "2000-01-01T00:00:00Z".into();
        Ok(())
    })
    .expect("deadline passed");
    assert!(
        remove(&b, &gb, &id).is_err(),
        "a local deadline cannot prove that approval was refused"
    );
    assert!(!crate::connect_groups::retired(&b, &id).expect("still reconciling"));
    update(&b, |s| {
        s.relations[0].state = DirectState::Expired;
        Ok(())
    })
    .expect("receiver confirmed expiry");
    assert!(crate::connect_groups::retired(&b, &id).expect("terminal decision"));
    remove(&b, &gb, &id).expect("confirmed refusal can be removed");
    assert!(load(&b).expect("store").retired.contains(&id));
}
#[test]
fn group_replacement_retires_authority_and_active_links_require_revocation_before_removal() {
    let (_temp, a, b, ga, _, id) = pair();
    approve(&a, &b, &ga, &id);
    update(&a, |s| {
        s.relations[0].expires_at = "2000-01-01T00:00:00Z".into();
        Ok(())
    })
    .expect("expire invitation");
    assert!(remove(&a, &ga, &id).is_err());
    let peer = InstanceIdentity::load(&b).expect("key").peer_id;
    assert!(binding(&a, &peer, &id).is_ok());
    let store = GroupStore::new(a.clone()).expect("store");
    let mut group = store.load(&ga).expect("group");
    group.generation = uuid::Uuid::new_v4().to_string();
    store.save(&group).expect("replace");
    assert!(binding(&a, &peer, &id).is_err());
}
#[test]
fn endpoint_validation_does_not_turn_invites_into_web_urls() {
    for value in ["192.168.1.2:8847", "[::1]:8847", "host.example:8847"] {
        assert!(address(value).is_ok(), "{value}");
    }
    for value in [
        "host",
        "https://host:443",
        "user:pass@host:8847",
        "host:8847/path",
        "host:8847?token=x",
        "host:0",
        "0.0.0.0:8847",
        "[::]:8847",
    ] {
        assert!(address(value).is_err(), "{value}");
    }
}

#[test]
fn removing_a_connection_does_not_make_its_invitation_reusable() {
    let (_temp, a, b, ga, gb, id) = pair();
    approve(&a, &b, &ga, &id);
    let invitation = load(&b).expect("store").relations[0]
        .invitation
        .clone()
        .expect("invitation");
    let text = format!(
        "cccc-direct:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&invitation).expect("json"))
    );
    set_state(&b, &gb, &id, false).expect("revoke");
    remove(&b, &gb, &id).expect("remove");
    assert!(load(&b).expect("store").relations.is_empty());
    assert!(join(&b, &gb, &text).is_err());
    assert!(binding(&a, &InstanceIdentity::load(&b).expect("key").peer_id, &id).is_ok());
}
