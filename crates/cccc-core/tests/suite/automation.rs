// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, Event, GroupState};
use cccc_core::{GroupStore, HomeLayout, actors, automation, ledger};
use serde_json::json;
use std::collections::HashSet;

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
fn mail_notice_waits_for_a_delivery_eligible_actor_and_is_one_shot() {
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
                .insert("delivery".into(), json!({"mail_notice_after_seconds":1}));
            Ok(())
        })
        .expect("delivery settings");
    let mut message = Event::new("chat.message", &group.group_id);
    message.by = "user".into();
    message.ts = "2020-01-01T00:00:00Z".into();
    message.data = json!({
        "text":"private work detail that must not be copied into a notice",
        "to":["peer"],
        "message_mode":"mail"
    })
    .as_object()
    .cloned()
    .expect("message");
    ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger"),
        &message,
    )
    .expect("append unread message");

    let none =
        automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &HashSet::new())
            .expect("stopped actor tick");
    assert!(none.notifications.is_empty());

    let eligible = HashSet::from(["peer".to_owned()]);
    let due = automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
        .expect("running actor tick");
    assert_eq!(due.notifications.len(), 1);
    assert_eq!(due.notifications[0].data["kind"], "mail_notice");
    assert_eq!(due.notifications[0].data["context"]["count"], 1);
    assert!(
        !due.notifications[0].data["message"]
            .as_str()
            .unwrap_or_default()
            .contains("private work detail")
    );

    let repeated =
        automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
            .expect("repeated tick");
    assert!(repeated.notifications.is_empty());
}

#[test]
fn actor_start_begins_a_fresh_mail_notice_window() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("actor resume window", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.state = GroupState::Active;
            actors::add(group, Actor::new("peer"))?;
            group
                .extra
                .insert("delivery".into(), json!({"mail_notice_after_seconds":60}));
            Ok(())
        })
        .expect("delivery settings");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
    let mut message = Event::new("chat.message", &group.group_id);
    message.by = "user".into();
    message.ts = "2020-01-01T00:00:00Z".into();
    message.data = json!({
        "text":"old Mail","to":["peer"],"message_mode":"mail"
    })
    .as_object()
    .cloned()
    .expect("message");
    ledger::append(&ledger_path, &message).expect("append Mail");
    let mut started = Event::new("actor.start", &group.group_id);
    started.by = "user".into();
    started.data = json!({"actor_id":"peer","runner":"headless"})
        .as_object()
        .cloned()
        .expect("start data");
    ledger::append(&ledger_path, &started).expect("append actor start");

    let eligible = HashSet::from(["peer".to_owned()]);
    let tick = automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
        .expect("post-start tick");
    assert!(
        tick.notifications.is_empty(),
        "old Mail must wait for a fresh notice window after actor.start"
    );
}

#[test]
fn mail_arriving_before_batch_closure_shares_the_existing_notice() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("evolving mail batch", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.state = GroupState::Active;
            actors::add(group, Actor::new("peer"))?;
            group
                .extra
                .insert("delivery".into(), json!({"mail_notice_after_seconds":1}));
            Ok(())
        })
        .expect("delivery settings");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
    let append_mail = |text: &str| {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.ts = "2020-01-01T00:00:00Z".into();
        event.data = json!({"text":text,"to":["peer"],"message_mode":"mail"})
            .as_object()
            .cloned()
            .expect("mail data");
        ledger::append(&ledger_path, &event).expect("append mail");
        event
    };
    let append_reply = |source: &Event| {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "peer".into();
        event.data = json!({
            "text":"handled","to":["user"],"message_mode":"send","reply_to":source.id
        })
        .as_object()
        .cloned()
        .expect("reply data");
        ledger::append(&ledger_path, &event).expect("append reply");
    };
    let eligible = HashSet::from(["peer".to_owned()]);

    let first = append_mail("first batch item");
    let initial =
        automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
            .expect("initial notice");
    assert_eq!(initial.notifications.len(), 1);

    let joined = append_mail("joined before closure");
    append_reply(&first);
    let same_batch =
        automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
            .expect("same batch tick");
    assert!(
        same_batch.notifications.is_empty(),
        "Mail that arrived before the original batch closed must not create another prompt"
    );

    append_reply(&joined);
    let next = append_mail("next batch item");
    let next_batch =
        automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
            .expect("next batch notice");
    assert_eq!(next_batch.notifications.len(), 1);
    assert_eq!(
        next_batch.notifications[0].data["context"]["source_event_ids"],
        json!([next.id])
    );
}

#[test]
fn reply_notice_starts_only_after_delivery_acceptance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("automation precedence", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.state = GroupState::Active;
            actors::add(group, Actor::new("peer"))?;
            group
                .extra
                .insert("delivery".into(), json!({"reply_notice_after_seconds":1}));
            Ok(())
        })
        .expect("automation config");
    let mut message = Event::new("chat.message", &group.group_id);
    message.by = "user".into();
    message.ts = "2020-01-01T00:00:00Z".into();
    message.data = json!({
        "text":"please answer",
        "to":["peer"],
        "message_mode":"request_reply"
    })
    .as_object()
    .cloned()
    .expect("message");
    ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger"),
        &message,
    )
    .expect("append unread message");

    let eligible = HashSet::from(["peer".to_owned()]);
    let before_acceptance =
        automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
            .expect("pre-acceptance tick");
    assert!(before_acceptance.notifications.is_empty());

    let mut accepted = Event::new("runtime.delivery", &group.group_id);
    accepted.by = "system".into();
    accepted.ts = "2020-01-01T00:00:01Z".into();
    accepted.data = json!({
        "source_event_id":message.id,
        "actor_id":"peer",
        "state":"accepted"
    })
    .as_object()
    .cloned()
    .expect("delivery fact");
    ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger"),
        &accepted,
    )
    .expect("append accepted delivery");

    let due = automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
        .expect("reply notice tick");
    assert_eq!(due.notifications.len(), 1);
    assert_eq!(due.notifications[0].data["kind"], "reply_notice");
    let repeated =
        automation::tick_group_for_delivery_actors(&home, &group.group_id, true, &eligible)
            .expect("repeated tick");
    assert!(repeated.notifications.is_empty());
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
