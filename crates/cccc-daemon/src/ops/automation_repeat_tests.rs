use super::*;
use cccc_contracts::{Actor, DaemonRequest, GroupState};
use cccc_core::{GroupStore, HomeLayout, actors, ledger};
use serde_json::{Value, json};

fn state(store: &GroupStore, group_id: &str) -> Value {
    cccc_core::fs::read_json(
        &store
            .state_dir(group_id)
            .expect("state dir")
            .join("automation.json"),
    )
    .expect("read state")
}

#[test]
fn captured_repeating_rules_complete_and_keep_count_until_deleted() {
    let captured: Value = serde_json::from_str(include_str!(
        "../../../cccc-core/tests/fixtures/automation-repeat-capture.json"
    ))
    .expect("captured rules");
    for kind in ["interval", "cron"] {
        let temp = tempfile::tempdir().expect("temp dir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("repeat", "").expect("group");
        let mut rule = captured[kind].clone();
        // Add the new option and replace private identities/message with test labels.
        rule["id"] = json!("repeat");
        rule["enabled"] = json!(true);
        rule["scope"] = json!("group");
        rule["to"] = json!(["peer"]);
        rule["action"]["message"] = json!("[REDACTED]");
        rule["trigger"]["max_runs"] = json!(2);
        store
            .mutate(&group.group_id, |group| {
                group.state = GroupState::Active;
                actors::add(group, Actor::new("peer"))?;
                group.automation = json!({"version":1,"rules":[rule.clone()]})
                    .as_object()
                    .expect("rule object")
                    .clone();
                Ok(())
            })
            .expect("configure group");
        let due = || {
            cccc_core::automation::mark_rule_fired(
                &home,
                &group.group_id,
                "repeat",
                chrono::Utc::now().timestamp() - 86400,
            )
            .expect("set due time")
        };
        let tick = || tick_group(&home, &group.group_id, false, &AtomicBool::new(false));
        let manage = |action: Value| {
            call(
                &home,
                "group_automation_manage",
                json!({
                    "group_id":group.group_id,"by":"user","actions":[action]
                }),
            )
        };
        due();
        tick();
        assert_eq!(
            state(&store, &group.group_id)["rules"]["repeat"]["fired_count"],
            1
        );
        assert!(manage(
            json!({"type":"set_rule_enabled","rule_id":"repeat","enabled":false})
        ));
        rule["trigger"]["max_runs"] = json!(3);
        rule["enabled"] = json!(false);
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        assert_eq!(
            state(&store, &group.group_id)["rules"]["repeat"]["fired_count"],
            1
        );
        rule["trigger"]["max_runs"] = json!(2);
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        assert!(manage(
            json!({"type":"set_rule_enabled","rule_id":"repeat","enabled":true})
        ));
        // Resume rebases the captured schedule instead of catching up.
        due();
        cccc_core::automation::reset_rule_timers_on_resume(&home, &group.group_id)
            .expect("resume timers");
        tick();
        assert_eq!(
            state(&store, &group.group_id)["rules"]["repeat"]["fired_count"],
            1
        );
        due();
        tick();
        assert_eq!(
            state(&store, &group.group_id)["rules"]["repeat"]["fired_count"],
            2
        );
        assert_eq!(
            store.load(&group.group_id).expect("load group").automation["rules"][0]["enabled"],
            false
        );
        let response = dispatch(
            &home,
            &DaemonRequest {
                v: 1,
                op: "group_automation_state".into(),
                args: json!({"group_id":group.group_id,"by":"user"})
                    .as_object()
                    .expect("request object")
                    .clone(),
            },
        );
        assert!(response.ok);
        assert_eq!(response.result["status"]["repeat"]["completed"], true);
        assert!(manage(
            json!({"type":"set_rule_enabled","rule_id":"repeat","enabled":true})
        ));
        due();
        tick();
        let events = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("read ledger");
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == "system.notify" && e.data["context"]["rule_id"] == "repeat")
                .count(),
            2
        );
        assert!(manage(json!({"type":"delete_rule","rule_id":"repeat"})));
        assert!(
            state(&store, &group.group_id)["rules"]
                .get("repeat")
                .is_none()
        );
    }
}

#[test]
fn unlimited_firings_do_not_count_and_removing_a_limit_clears_it_on_save() {
    let captured: Value = serde_json::from_str(include_str!(
        "../../../cccc-core/tests/fixtures/automation-repeat-capture.json"
    ))
    .expect("captured rules");
    for kind in ["interval", "cron"] {
        let temp = tempfile::tempdir().expect("temp dir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("repeat", "").expect("group");
        let mut rule = captured[kind].clone();
        rule["id"] = json!("repeat");
        rule["enabled"] = json!(true);
        rule["scope"] = json!("group");
        rule["to"] = json!(["peer"]);
        rule["action"]["message"] = json!("hello");
        store
            .mutate(&group.group_id, |group| {
                group.state = GroupState::Active;
                actors::add(group, Actor::new("peer"))?;
                group.automation = json!({"version":1,"rules":[rule.clone()]})
                    .as_object()
                    .expect("rule object")
                    .clone();
                Ok(())
            })
            .expect("configure group");
        let due_and_tick = || {
            cccc_core::automation::mark_rule_fired(
                &home,
                &group.group_id,
                "repeat",
                chrono::Utc::now().timestamp() - 86400,
            )
            .expect("set due time");
            tick_group(&home, &group.group_id, false, &AtomicBool::new(false));
        };
        let manage = |action: Value| {
            call(
                &home,
                "group_automation_manage",
                json!({"group_id":group.group_id,"by":"user","actions":[action]}),
            )
        };
        let status = || {
            let response = dispatch(
                &home,
                &DaemonRequest {
                    v: 1,
                    op: "group_automation_state".into(),
                    args: json!({"group_id":group.group_id,"by":"user"})
                        .as_object()
                        .expect("request object")
                        .clone(),
                },
            );
            assert!(response.ok);
            response.result["status"]["repeat"].clone()
        };

        due_and_tick();
        due_and_tick();
        assert!(
            state(&store, &group.group_id)["rules"]["repeat"]
                .get("fired_count")
                .is_none()
        );

        rule["trigger"]["max_runs"] = json!(3);
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        due_and_tick();
        assert_eq!(status()["fired_count"], 1);
        rule["trigger"]["max_runs"] = json!(1);
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        assert_eq!(status()["completed"], true);
        assert_eq!(status()["completed_at"], "");
        rule["trigger"]["max_runs"] = json!(3);
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        assert_eq!(status()["completed"], false);
        assert_eq!(status()["fired_count"], 1);

        rule["enabled"] = json!(false);
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        rule["trigger"]
            .as_object_mut()
            .expect("trigger")
            .remove("max_runs");
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        assert!(
            state(&store, &group.group_id)["rules"]["repeat"]
                .get("fired_count")
                .is_none()
        );
        rule["trigger"]["max_runs"] = json!(1);
        assert!(manage(json!({"type":"update_rule","rule":rule})));
        assert_eq!(status()["fired_count"], 0);
        assert_eq!(status()["completed"], false);
        assert!(manage(
            json!({"type":"set_rule_enabled","rule_id":"repeat","enabled":true})
        ));
        due_and_tick();
        assert_eq!(status()["fired_count"], 1);
        assert_eq!(status()["completed"], true);
        let events = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
            .expect("read ledger");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "system.notify")
                .count(),
            4
        );
    }
}
