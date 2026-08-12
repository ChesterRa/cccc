// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, Event, GroupState};
use cccc_core::{GroupStore, HomeLayout, actors, automation, ledger};
use serde_json::json;

#[test]
fn canonical_interval_rule_starts_its_clock_and_emits_once_when_due() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("automation", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            actors::add(group, Actor::new("peer"))?;
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
    assert!(first.notifications.is_empty());
    let state_path = store
        .state_dir(&group.group_id)
        .expect("state dir")
        .join("automation.json");
    let mut state: serde_json::Value =
        cccc_core::fs::read_json(&state_path).expect("automation state");
    state["rules"]["reminder"]["last_fired_at"] = json!("2020-01-01T00:00:00Z");
    cccc_core::fs::write_json(&state_path, &state).expect("due state");

    let due = automation::tick(&home).expect("due tick");
    assert_eq!(due.notifications.len(), 1);
    assert_eq!(due.notifications[0].data["message"], "check in");
    let repeated = automation::tick(&home).expect("repeated tick");
    assert!(repeated.notifications.is_empty());
}

#[test]
fn idle_group_suppresses_builtin_standup_but_runs_custom_rules() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("idle automation", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.state = GroupState::Idle;
            actors::add(group, Actor::new("peer"))?;
            group.automation = json!({
                "version":1,
                "rules":[
                    {
                        "id":"standup","enabled":true,"to":["@all"],
                        "trigger":{"kind":"interval","every_seconds":60},
                        "action":{"kind":"notify","message":"built in"}
                    },
                    {
                        "id":"custom","enabled":true,"to":["@all"],
                        "trigger":{"kind":"interval","every_seconds":60},
                        "action":{"kind":"notify","message":"custom"}
                    }
                ]
            })
            .as_object()
            .cloned()
            .expect("object");
            Ok(())
        })
        .expect("automation config");

    let baseline = automation::tick_group(&home, &group.group_id, false).expect("baseline tick");
    assert!(baseline.notifications.is_empty());
    let state_path = store
        .state_dir(&group.group_id)
        .expect("state dir")
        .join("automation.json");
    let mut state: serde_json::Value =
        cccc_core::fs::read_json(&state_path).expect("automation state");
    state["rules"]["standup"]["last_fired_at"] = json!("2020-01-01T00:00:00Z");
    state["rules"]["custom"]["last_fired_at"] = json!("2020-01-01T00:00:00Z");
    cccc_core::fs::write_json(&state_path, &state).expect("due state");

    let due = automation::tick_group(&home, &group.group_id, false).expect("idle tick");
    assert_eq!(due.notifications.len(), 1);
    assert_eq!(due.notifications[0].data["context"]["rule_id"], "custom");
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

#[test]
fn canonical_automation_timing_precedes_legacy_flat_setting() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("automation precedence", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.state = GroupState::Active;
            actors::add(group, Actor::new("peer"))?;
            group
                .automation
                .insert("unread_nudge_after_seconds".into(), json!(1));
            group
                .extra
                .insert("settings".into(), json!({"unread_nudge_after_seconds":0}));
            Ok(())
        })
        .expect("automation config");
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

    let result = automation::tick(&home).expect("automation tick");
    assert_eq!(result.notifications.len(), 1);
    assert_eq!(result.notifications[0].data["kind"], "unread_nudge");
}

#[test]
fn scheduled_action_remains_due_until_its_owner_confirms_completion() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("automation action", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.automation = json!({
                "version":1,
                "rules":[{
                    "id":"pause-once","enabled":true,"scope":"group",
                    "trigger":{"kind":"at","at":"2020-01-01T00:00:00Z"},
                    "action":{"kind":"group_state","state":"paused"}
                }]
            })
            .as_object()
            .cloned()
            .expect("automation");
            Ok(())
        })
        .expect("automation rule");

    let first = automation::tick_group(&home, &group.group_id, false).expect("first tick");
    assert_eq!(first.actions.len(), 1);
    let unconfirmed =
        automation::tick_group(&home, &group.group_id, false).expect("unconfirmed tick");
    assert_eq!(
        unconfirmed.actions.len(),
        1,
        "returning an action is not proof that the daemon applied it"
    );
}
