// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, ActorRole, Event};
use cccc_core::actors;
use cccc_core::context::{ContextDoc, ContextStore};
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

    let dry = json!({"op": "coordination.brief.update", "objective": "dry"})
        .as_object()
        .cloned()
        .expect("operation");
    contexts
        .sync(&group_id, &[dry], Some(&first.version), "user", true)
        .expect("dry run");
    let stored = contexts.load(&group_id).expect("stored context");
    assert_ne!(
        stored
            .coordination
            .get("brief")
            .and_then(|brief| brief.get("objective")),
        Some(&json!("dry"))
    );

    let valid_then_invalid = [
        json!({"op":"task.update","task_id":"T001","notes":"must roll back"})
            .as_object()
            .cloned()
            .expect("valid update"),
        json!({"op":"task.move","task_id":"T001","status":"bogus"})
            .as_object()
            .cloned()
            .expect("invalid move"),
    ];
    assert!(
        contexts
            .sync(
                &group_id,
                &valid_then_invalid,
                Some(&first.version),
                "user",
                false,
            )
            .is_err()
    );
    let stored = contexts.load(&group_id).expect("stored after rejection");
    assert!(stored.tasks[0].get("notes").is_none());
    assert_eq!(stored.tasks[0]["status"], "planned");
}

#[test]
fn stored_coordination_notes_survive_brief_and_task_writes() {
    // Coordination notes were removed. Notes left in an older context file are
    // never read again, but later writes keep them in the file unchanged.
    let (_temp, home, _store, group_id) = fixture();
    let contexts = ContextStore::new(home.clone()).expect("contexts");
    let brief = |objective: &str| {
        json!({"op": "coordination.brief.update", "objective": objective})
            .as_object()
            .cloned()
            .expect("operation")
    };
    contexts
        .sync(&group_id, &[brief("first")], None, "user", false)
        .expect("first write");
    let context_file = home
        .groups_dir()
        .join(&group_id)
        .join("context/context.yaml");
    std::fs::write(
        &context_file,
        "coordination:\n  brief:\n    objective: first\n  recent_decisions:\n  - at: '2026-10-01T00:00:00Z'\n    by: user\n    summary: old decision\n    task_id: null\n  recent_handoffs:\n  - at: '2026-10-02T00:00:00Z'\n    by: peer\n    summary: old handoff\n    task_id: T001\nmeta: {}\n",
    )
    .expect("seed older notes");
    let notes = |document: &ContextDoc| {
        (
            document.coordination.get("recent_decisions").cloned(),
            document.coordination.get("recent_handoffs").cloned(),
        )
    };
    let before = notes(&contexts.load(&group_id).expect("seeded context"));
    assert!(before.0.is_some() && before.1.is_some(), "{before:?}");

    contexts
        .sync(&group_id, &[brief("second")], None, "user", false)
        .expect("brief write");
    let task = json!({"op": "task.create", "title": "after notes"})
        .as_object()
        .cloned()
        .expect("operation");
    contexts
        .sync(&group_id, &[task], None, "user", false)
        .expect("task write");

    let stored = contexts.load(&group_id).expect("stored context");
    assert_eq!(notes(&stored), before);
    assert_eq!(stored.coordination["brief"]["objective"], "second");
    assert_eq!(stored.tasks.len(), 1);
}

#[test]
fn legacy_context_json_is_migrated_once_without_deleting_the_source() {
    let temp = tempfile::tempdir().expect("temp home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("migration", "").expect("group");
    let group_dir = groups.group_dir(&group.group_id).expect("group dir");
    let mut legacy = ContextDoc::default();
    legacy.tasks.push(
        json!({"id":"t_legacy","title":"legacy Rust task","status":"done"})
            .as_object()
            .cloned()
            .expect("task"),
    );
    std::fs::write(
        group_dir.join("state/context.json"),
        serde_json::to_vec_pretty(&legacy).expect("legacy JSON"),
    )
    .expect("write legacy");

    let contexts = ContextStore::new(home).expect("contexts");
    let first = contexts.load(&group.group_id).expect("migrated context");
    let second = contexts.load(&group.group_id).expect("idempotent context");
    assert_eq!(first.tasks, second.tasks);
    assert_eq!(first.tasks.len(), 1);
    assert_eq!(first.tasks[0]["id"], "T001");
    assert!(group_dir.join("context/tasks/T001.yaml").is_file());
    assert!(
        group_dir
            .join("context/.rust-state-migrated-v1.json")
            .is_file()
    );
    assert!(group_dir.join("state/context.json").is_file());
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
    message.data = json!({"text": "hello", "to": ["peer"], "message_mode": "mail"})
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
    let consumed = inbox::consume_unread(&home, &group, "peer", "peer", 50).expect("consume Mail");
    assert_eq!(consumed.messages, vec![message]);
    assert_eq!(consumed.read_event.expect("mail.read").kind, "mail.read");
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
            let mut secretary = Actor::new("fixture-internal");
            secretary.internal_kind = Some("fixture_helper".into());
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
        "fixture-internal"
    ));
    assert!(!inbox::is_for_actor(
        &group,
        &message(&["@all"]),
        "fixture-internal"
    ));
    assert!(!inbox::is_for_actor(
        &group,
        &message(&[]),
        "fixture-internal"
    ));
    assert!(inbox::is_for_actor(
        &group,
        &message(&["fixture-internal"]),
        "fixture-internal"
    ));

    let mut notify = Event::new("system.notify", &group_id);
    notify.by = "system".into();
    notify.data = json!({"actor_id":"fixture-internal","text":"wake up"})
        .as_object()
        .cloned()
        .expect("notify data");
    assert!(inbox::is_for_actor(&group, &notify, "fixture-internal"));
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
