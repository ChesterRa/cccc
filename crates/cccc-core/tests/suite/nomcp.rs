// Included by the crate-level integration test harness.
use cccc_core::nomcp::{CreateSpec, Store};
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};
use serde_json::{Value, json};

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
    let store = Store::new(home.clone()).expect("store");
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
    let session_path = home
        .root()
        .join("state/nomcp_sessions")
        .join(format!("{}.json", created.session.sid));
    let canonical: Value =
        serde_json::from_slice(&std::fs::read(&session_path).expect("read canonical session"))
            .expect("parse canonical session");
    assert!(
        canonical["token_hash"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(canonical.get("secret_sha256").is_none());
    assert_eq!(canonical["repo_root"].as_str(), temp.path().to_str());
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

#[test]
fn legacy_rust_secret_migrates_without_erasing_python_metadata() {
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
    let store = Store::new(home.clone()).expect("store");
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
    let session_path = home
        .root()
        .join("state/nomcp_sessions")
        .join(format!("{}.json", created.session.sid));
    let mut legacy: Value =
        serde_json::from_slice(&std::fs::read(&session_path).expect("read session"))
            .expect("parse session");
    let object = legacy.as_object_mut().expect("session object");
    let digest = object.remove("token_hash").expect("canonical digest");
    object.insert("secret_sha256".into(), digest);
    object.insert(
        "sent_messages".into(),
        json!({"python-message":{"event_id":"evt-python","at":"2026-08-10T00:00:00Z","via":"get"}}),
    );
    object.insert("created_status_digest".into(), json!("python-status"));
    cccc_core::fs::write_json(&session_path, &legacy).expect("write legacy session");

    assert!(
        store
            .authorize(&created.session.sid, &created.secret)
            .is_ok()
    );
    assert!(
        store
            .record_message(&created.session.sid, "rust-message")
            .expect("record")
    );

    let migrated: Value =
        serde_json::from_slice(&std::fs::read(&session_path).expect("read migrated session"))
            .expect("parse migrated session");
    assert!(
        migrated["token_hash"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(migrated.get("secret_sha256").is_none());
    assert_eq!(
        migrated["sent_messages"]["python-message"]["event_id"],
        "evt-python"
    );
    assert_eq!(migrated["created_status_digest"], "python-status");
    assert_eq!(
        migrated["sent_message_ids"],
        json!(["python-message", "rust-message"])
    );
}
