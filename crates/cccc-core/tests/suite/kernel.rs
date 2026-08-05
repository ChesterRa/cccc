// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, ActorRole, Event};
use cccc_core::actors;
use cccc_core::context::ContextStore;
use cccc_core::inbox;
use cccc_core::ledger;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

fn fixture() -> (tempfile::TempDir, HomeLayout, GroupStore, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("test", "").expect("group");
    let group_id = group.group_id;
    (temp, home, store, group_id)
}

#[test]
fn group_lifecycle_stays_inside_rust_home() {
    let (_temp, home, store, group_id) = fixture();
    assert!(home.groups_dir().join(&group_id).is_dir());
    assert_eq!(store.list().expect("list").len(), 1);
    assert!(store.load("../.cccc").is_err());
    assert!(store.delete(&group_id).expect("delete"));
    assert!(store.list().expect("list after delete").is_empty());
}

#[test]
fn actor_order_defines_stable_roles() {
    let (_temp, _home, store, group_id) = fixture();
    store
        .mutate(&group_id, |group| {
            actors::add(group, Actor::new("lead"))?;
            actors::add(group, Actor::new("peer"))?;
            Ok(())
        })
        .expect("add actors");
    let group = store.load(&group_id).expect("load");
    assert_eq!(
        actors::effective_role(&group, "lead"),
        Some(ActorRole::Foreman)
    );
    assert_eq!(
        actors::effective_role(&group, "peer"),
        Some(ActorRole::Peer)
    );
}

#[test]
fn context_sync_is_atomic_and_dry_run_does_not_persist() {
    let (_temp, home, _store, group_id) = fixture();
    let contexts = ContextStore::new(home).expect("contexts");
    let create = json!({"op": "task.create", "title": "Port kernel"})
        .as_object()
        .cloned()
        .expect("operation");
    let first = contexts
        .sync(&group_id, &[create], None, "user", false)
        .expect("sync");
    assert_eq!(first.context.tasks.len(), 1);

    let note = json!({"op": "coordination.note.add", "kind": "decision", "summary": "dry"})
        .as_object()
        .cloned()
        .expect("operation");
    contexts
        .sync(&group_id, &[note], Some(&first.version), "user", true)
        .expect("dry run");
    let stored = contexts.load(&group_id).expect("stored context");
    assert!(stored.coordination.get("notes").is_none());
}

#[test]
fn inbox_filters_targeted_messages_and_persists_cursor() {
    let (_temp, home, store, group_id) = fixture();
    store
        .mutate(&group_id, |group| {
            actors::add(group, Actor::new("lead"))?;
            actors::add(group, Actor::new("peer"))?;
            Ok(())
        })
        .expect("actors");
    let mut message = Event::new("chat.message", &group_id);
    message.by = "lead".into();
    message.data = json!({"text": "hello", "to": ["peer"]})
        .as_object()
        .cloned()
        .unwrap_or_else(Map::<String, Value>::new);
    ledger::append(&store.ledger_path(&group_id).expect("path"), &message).expect("append");
    let group = store.load(&group_id).expect("load");
    let unread = inbox::list_unread(&home, &group, "peer", 50).expect("unread");
    assert_eq!(unread.len(), 1);
    let unread_many =
        inbox::list_unread_many(&home, &group, &["lead".to_owned(), "peer".to_owned()], 50)
            .expect("batch unread");
    assert!(unread_many["lead"].is_empty());
    assert_eq!(unread_many["peer"], unread);
    inbox::mark_read(&home, &group_id, "peer", &message.id).expect("mark read");
    assert!(
        inbox::list_unread(&home, &group, "peer", 50)
            .expect("read inbox")
            .is_empty()
    );
}

#[test]
fn internal_assistant_is_not_a_peer_recipient() {
    let (_temp, _home, store, group_id) = fixture();
    store
        .mutate(&group_id, |group| {
            actors::add(group, Actor::new("lead"))?;
            actors::add(group, Actor::new("peer"))?;
            let mut secretary = Actor::new("voice-secretary");
            secretary.internal_kind = Some("voice_secretary".into());
            actors::add(group, secretary)?;
            Ok(())
        })
        .expect("actors");
    let group = store.load(&group_id).expect("load");
    let message = |to: &[&str]| {
        let mut event = Event::new("chat.message", &group_id);
        event.by = "lead".into();
        event.data = json!({"text":"hello","to":to})
            .as_object()
            .cloned()
            .expect("message data");
        event
    };

    assert!(inbox::is_for_actor(&group, &message(&["@peers"]), "peer"));
    assert!(!inbox::is_for_actor(
        &group,
        &message(&["@peers"]),
        "voice-secretary"
    ));
    assert!(!inbox::is_for_actor(
        &group,
        &message(&["@all"]),
        "voice-secretary"
    ));
    assert!(!inbox::is_for_actor(
        &group,
        &message(&[]),
        "voice-secretary"
    ));
    assert!(inbox::is_for_actor(
        &group,
        &message(&["voice-secretary"]),
        "voice-secretary"
    ));

    let mut notify = Event::new("system.notify", &group_id);
    notify.by = "system".into();
    notify.data = json!({"actor_id":"voice-secretary","text":"wake up"})
        .as_object()
        .cloned()
        .expect("notify data");
    assert!(inbox::is_for_actor(&group, &notify, "voice-secretary"));
}

#[test]
fn system_notification_respects_its_explicit_actor_target() {
    let (_temp, _home, store, group_id) = fixture();
    store
        .mutate(&group_id, |group| {
            actors::add(group, Actor::new("lead"))?;
            actors::add(group, Actor::new("peer"))?;
            Ok(())
        })
        .expect("actors");
    let group = store.load(&group_id).expect("load");
    let mut notification = Event::new("system.notify", &group_id);
    notification.by = "system".into();
    notification.data = json!({
        "target_actor_id": "peer",
        "title": "New message",
        "message": "Check your inbox."
    })
    .as_object()
    .cloned()
    .expect("notification data");

    assert!(inbox::is_for_actor(&group, &notification, "peer"));
    assert!(!inbox::is_for_actor(&group, &notification, "lead"));
}

#[test]
fn legacy_chat_notice_is_folded_into_its_source_message() {
    let (_temp, _home, store, group_id) = fixture();
    store
        .mutate(&group_id, |group| actors::add(group, Actor::new("peer")))
        .expect("actor");
    let group = store.load(&group_id).expect("load");
    let mut notification = Event::new("system.notify", &group_id);
    notification.by = "system".into();
    notification.data = json!({
        "target_actor_id": "peer",
        "title": "New message",
        "message": "New message from user. Check your inbox.",
        "context": {"event_id": "source-message", "from": "user"}
    })
    .as_object()
    .cloned()
    .expect("notification data");

    assert!(!inbox::is_for_actor(&group, &notification, "peer"));
}
