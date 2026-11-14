#[path = "direct_shared_permanent_tests.rs"]
mod permanent;

use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: String,
    guest: HomeLayout,
    groups: Vec<String>,
    text: String,
}

impl Fixture {
    fn new() -> Self {
        Self::with_lifetime(604_800)
    }
    fn with_lifetime(seconds: i64) -> Self {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("host")).expect("home");
        let guest = HomeLayout::from_path(temp.path().join("guest")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("Host", "")
            .expect("group")
            .group_id;
        let store = GroupStore::new(guest.clone()).expect("store");
        let groups = (0..3)
            .map(|index| {
                store
                    .create(&format!("Guest {index}"), "")
                    .expect("group")
                    .group_id
            })
            .collect();
        super::super::configure(
            &home,
            Some(DirectListener {
                bind: "127.0.0.1:8847".into(),
                address: "127.0.0.1:8847".into(),
            }),
            None,
        )
        .expect("metadata only");
        let text = create(&home, &group, None, seconds).expect("shared invitation");
        Self {
            _temp: temp,
            home,
            group,
            guest,
            groups,
            text,
        }
    }

    fn join(&self, index: usize) -> DirectRelation {
        let id = super::super::join(&self.guest, &self.groups[index], &self.text).expect("join");
        load(&self.guest)
            .expect("store")
            .relations
            .into_iter()
            .find(|relation| relation.id == id)
            .expect("relation")
    }

    fn hello(&self, relation: &DirectRelation) -> io::Result<DirectState> {
        let invite = relation.invitation.as_ref().expect("invitation");
        hello(
            &self.home,
            &invite.id,
            &relation.id,
            &invite.secret,
            &relation.local,
            &relation.local.public_key,
        )
    }
}

#[test]
fn one_invitation_pairs_multiple_groups_idempotently_with_independent_approval() {
    let f = Fixture::new();
    let first = f.join(0);
    let second = f.join(1);
    assert_ne!(first.id, second.id);
    assert_eq!(f.join(0).id, first.id);
    GroupStore::new(f.guest.clone())
        .expect("store")
        .mutate(&f.groups[0], |group| {
            group.title = "Renamed Group".into();
            Ok(())
        })
        .expect("rename without replacing its identity");
    assert_eq!(
        f.join(0).id,
        first.id,
        "display metadata must not turn a replay into a new pairing"
    );
    for relation in [&first, &first, &second] {
        assert_eq!(f.hello(relation).expect("request"), DirectState::Pending);
    }
    let store = load(&f.home).expect("store");
    assert_eq!(
        store.shared_invitations[0].uses, 2,
        "retries must not consume another use"
    );
    assert_eq!(
        copy(
            &f.home,
            &f.group,
            &store.shared_invitations[0].invitation.id
        )
        .expect("copy after reload"),
        f.text
    );
    assert!(super::super::binding(&f.home, &first.local.instance_id, &first.id).is_err());
    super::super::set_state(&f.home, &f.group, &first.id, true).expect("approve first Group");
    assert!(super::super::binding(&f.home, &first.local.instance_id, &first.id).is_ok());
    assert!(super::super::binding(&f.home, &second.local.instance_id, &second.id).is_err());
}

#[test]
fn expiry_keeps_approved_connections_but_key_deletion_revokes_all_grants() {
    let f = Fixture::new();
    let active = f.join(0);
    let pending = f.join(1);
    let unused = f.join(2);
    f.hello(&active).expect("request");
    f.hello(&pending).expect("request");
    super::super::set_state(&f.home, &f.group, &active.id, true).expect("approve");
    update(&f.home, |store| {
        store.shared_invitations[0].invitation.expires_at = "2000-01-01T00:00:00Z".into();
        for relation in &mut store.relations {
            relation.expires_at = "2000-01-01T00:00:00Z".into();
        }
        Ok(())
    })
    .expect("move the deadline, no sleeping or wall-clock assertion");
    assert_eq!(
        f.hello(&active).expect("approved peer reconnects"),
        DirectState::Active
    );
    assert_eq!(
        f.hello(&unused).expect("expired invitation"),
        DirectState::Expired
    );
    assert!(super::super::set_state(&f.home, &f.group, &pending.id, true).is_err());
    let root = active.invitation.as_ref().expect("invitation").id.clone();
    assert!(copy(&f.home, &f.group, &root).is_err());
    revoke(&f.home, &f.group, &root).expect("delete invitation");
    assert_eq!(
        f.hello(&unused).expect("deleted invitation"),
        DirectState::Revoked
    );
    assert_eq!(
        f.hello(&pending).expect("pending cancelled"),
        DirectState::Revoked
    );
    assert_eq!(
        f.hello(&active).expect("deleted key cannot reconnect"),
        DirectState::Revoked
    );
    assert!(super::super::binding(&f.home, &active.local.instance_id, &active.id).is_err());
}

#[test]
fn forged_requests_wrong_group_and_invalid_lifetimes_do_not_change_grants() {
    let f = Fixture::new();
    let relation = f.join(0);
    let invite = relation.invitation.as_ref().expect("invitation");
    let before = load(&f.home).expect("store");
    assert!(
        hello(
            &f.home,
            &invite.id,
            &relation.id,
            &invite.secret,
            &relation.local,
            "wrong TLS key"
        )
        .is_err()
    );
    assert!(
        hello(
            &f.home,
            &invite.id,
            &relation.id,
            "wrong secret",
            &relation.local,
            &relation.local.public_key
        )
        .is_err()
    );
    assert!(
        hello(
            &f.home,
            &invite.id,
            &invite.id,
            &invite.secret,
            &relation.local,
            &relation.local.public_key
        )
        .is_err()
    );
    assert!(copy(&f.home, "other-group", &invite.id).is_err());
    assert!(revoke(&f.home, "other-group", &invite.id).is_err());
    for seconds in [-1, 60, 31 * 86_400] {
        assert!(create(&f.home, &f.group, None, seconds).is_err());
    }
    assert!(load(&f.home).expect("unchanged store") == before);
    super::super::retire_group(&f.home, &f.group).expect("retire Group");
    assert!(copy(&f.home, &f.group, &invite.id).is_err());
    assert_eq!(
        f.hello(&relation).expect("retired invitation"),
        DirectState::Revoked
    );
}
