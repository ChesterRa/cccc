// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, Event, GroupState};
use cccc_core::{GroupStore, HomeLayout, actors, automation, ledger};
use serde_json::json;

#[test]
fn canonical_interval_rule_emits_once_per_interval() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("automation", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.automation = json!({
                "version":1,
                "rules":[{
                    "id":"reminder","enabled":true,"to":["@all"],
                    "trigger":{"kind":"interval","every_seconds":3600},
                    "action":{"kind":"notify","message":"check in"}
                }]
            })
            .as_object()
            .cloned()
            .expect("object");
            Ok(())
        })
        .expect("automation config");

    let first = automation::tick(&home).expect("first tick");
    assert_eq!(first.notifications.len(), 1);
    assert_eq!(first.notifications[0].data["text"], "check in");
    let second = automation::tick(&home).expect("second tick");
    assert!(second.notifications.is_empty());
}

#[test]
fn unread_nudge_defaults_off_and_can_be_enabled_explicitly() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("automation", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.state = GroupState::Active;
            actors::add(group, Actor::new("peer"))?;
            group
                .extra
                .insert("settings".into(), json!({"nudge_after_seconds":1}));
            Ok(())
        })
        .expect("legacy unread setting");
    let mut message = Event::new("chat.message", &group.group_id);
    message.by = "user".into();
    message.ts = "2020-01-01T00:00:00Z".into();
    message.data = json!({"text":"pending","to":["peer"]})
        .as_object()
        .cloned()
        .expect("message");
    ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger"),
        &message,
    )
    .expect("append unread message");

    let disabled = automation::tick(&home).expect("disabled automation tick");
    assert!(disabled.notifications.is_empty());

    store
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "settings".into(),
                json!({"nudge_after_seconds":1,"unread_nudge_after_seconds":1}),
            );
            Ok(())
        })
        .expect("enable unread nudge");
    let enabled = automation::tick(&home).expect("enabled automation tick");
    assert_eq!(enabled.notifications.len(), 1);
    assert_eq!(enabled.notifications[0].data["kind"], "unread_nudge");
}
