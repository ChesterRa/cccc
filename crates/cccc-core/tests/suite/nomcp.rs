// Included by the crate-level integration test harness.
use cccc_core::nomcp::{CreateSpec, Store};
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};

#[test]
fn session_secret_revoke_and_message_ids_are_persistent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group_store = GroupStore::new(home.clone()).expect("group store");
    let group = group_store.create("nomcp", "").expect("group");
    group_scope::attach(
        &group_store,
        &group.group_id,
        Scope {
            scope_key: "scope_test".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "test".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let store = Store::new(home).expect("store");
    let created = store
        .create(CreateSpec {
            group_id: group.group_id,
            title: "review".into(),
            brief: "brief".into(),
            reply_to_event_id: String::new(),
            recipient: "user".into(),
            scope_key: String::new(),
            allowed_paths: Vec::new(),
            expires_in_seconds: 3600,
        })
        .expect("create");
    assert!(store.authorize(&created.session.sid, "wrong").is_err());
    assert!(
        store
            .authorize(&created.session.sid, &created.secret)
            .is_ok()
    );
    assert!(
        store
            .record_message(&created.session.sid, "m1")
            .expect("first")
    );
    assert!(
        !store
            .record_message(&created.session.sid, "m1")
            .expect("repeat")
    );
    assert!(store.revoke(&created.session.sid).expect("revoke"));
    assert!(
        store
            .authorize(&created.session.sid, &created.secret)
            .is_err()
    );
}
