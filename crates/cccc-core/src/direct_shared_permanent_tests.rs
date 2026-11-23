use super::*;

#[test]
fn permanent_key_has_a_versioned_deadline_and_counts_only_new_valid_applications() {
    let f = Fixture::with_lifetime(0);
    let first = f.join(0);
    let invite = first.invitation.as_ref().expect("invitation");
    assert_eq!(invite.v, 3);
    assert!(invite.permanent() && invite.reusable());
    assert_eq!(invite.expires_at, DIRECT_PERMANENT_EXPIRY);
    assert!(unexpired(&invite.expires_at));
    assert_eq!(
        f.hello(&first).expect("first application"),
        DirectState::Pending
    );
    assert_eq!(f.hello(&first).expect("retry"), DirectState::Pending);
    super::super::super::set_state(&f.home, &f.group, &first.id, true).expect("approve");
    assert_eq!(f.hello(&first).expect("reconnect"), DirectState::Active);
    let second = f.join(1);
    assert!(
        hello(
            &f.home,
            &invite.id,
            &second.id,
            "incorrect",
            &second.local,
            &second.local.public_key
        )
        .is_err()
    );
    assert_eq!(
        load(&f.home)
            .expect("read persisted count")
            .shared_invitations[0]
            .uses,
        1
    );
    assert_eq!(
        f.hello(&second).expect("second application"),
        DirectState::Pending
    );
    assert_eq!(
        load(&f.home)
            .expect("read persisted count")
            .shared_invitations[0]
            .uses,
        2
    );
    assert_eq!(
        copy(&f.home, &f.group, &invite.id).expect("explicit copy after reload"),
        f.text
    );
    revoke(&f.home, &f.group, &invite.id).expect("delete key");
    for relation in [&first, &second] {
        assert_eq!(
            f.hello(relation).expect("deleted key"),
            DirectState::Revoked
        );
        assert!(
            super::super::super::binding(&f.home, &relation.local.instance_id, &relation.id)
                .is_err()
        );
    }
    assert!(copy(&f.home, &f.group, &invite.id).is_err());
    assert!(
        load(&f.home)
            .expect("deleted metadata")
            .shared_invitations
            .is_empty()
    );
}

#[test]
fn permanent_marker_cannot_be_smuggled_into_an_older_invitation_version() {
    let f = Fixture::with_lifetime(0);
    let relation = f.join(0);
    let original = relation.invitation.expect("invitation");
    for version in [1, 2] {
        let mut malformed = original.clone();
        malformed.v = version;
        let text = pairing::encode_invitation(&malformed).expect("serialize malformed invitation");
        assert!(super::super::super::join(&f.guest, &f.groups[1], &text).is_err());
    }
    let mut malformed = original;
    malformed.expires_at = "9999-01-01T00:00:00Z".into();
    let text = pairing::encode_invitation(&malformed).expect("serialize malformed invitation");
    assert!(super::super::super::join(&f.guest, &f.groups[1], &text).is_err());
    assert_eq!(
        load(&f.home).expect("no applications").shared_invitations[0].uses,
        0
    );
}

#[test]
fn deleted_key_cannot_return_but_a_fresh_key_can_request_new_approval_for_the_same_pair() {
    let f = Fixture::with_lifetime(0);
    let original = f.join(0);
    f.hello(&original).expect("request");
    super::super::super::set_state(&f.home, &f.group, &original.id, true).expect("approve");
    let old = original.invitation.as_ref().expect("old key");
    revoke(&f.home, &f.group, &old.id).expect("delete key");
    assert_eq!(
        f.hello(&original).expect("old key remains retired"),
        DirectState::Revoked
    );
    super::super::super::set_state(&f.guest, &f.groups[0], &original.id, false)
        .expect("client has learned the revocation");
    let fresh = create(&f.home, &f.group, None, 0).expect("new key");
    let id =
        super::super::super::join(&f.guest, &f.groups[0], &fresh).expect("new explicit request");
    assert_ne!(id, original.id);
    let relation = load(&f.guest)
        .expect("guest")
        .relations
        .into_iter()
        .find(|relation| relation.id == id)
        .expect("fresh relation");
    assert_eq!(f.hello(&relation).expect("fresh key"), DirectState::Pending);
    assert!(
        super::super::super::binding(&f.home, &relation.local.instance_id, &id).is_err(),
        "A new key still requires administrator approval"
    );
    assert_eq!(
        f.hello(&original).expect("old ID stays retired"),
        DirectState::Revoked
    );
}
