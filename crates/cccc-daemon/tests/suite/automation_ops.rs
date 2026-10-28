// Included by the crate-level integration test harness.
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn automation_contract_is_versioned_and_frontend_compatible() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = ok(&home, "group_create", json!({"title":"automation"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");

    let initial = ok(
        &home,
        "group_automation_state",
        json!({"group_id":group_id}),
    );
    assert!(initial.result["ruleset"]["rules"].is_array());
    assert!(
        initial.result["ruleset"]["snippets"]["standup"]
            .as_str()
            .is_some_and(|value| value.contains("{{interval_minutes}}"))
    );
    assert_eq!(initial.result["version"], 1);

    let updated = ok(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "expected_version":1,
            "ruleset":{
                "rules":[{
                    "id":"reminder","enabled":true,"to":["@all"],
                    "trigger":{"kind":"interval","every_seconds":60},
                    "action":{"kind":"notify","message":"check in"}
                }],
                "snippets":{}
            }
        }),
    );
    assert_eq!(updated.result["version"], 2);
    assert_eq!(updated.result["ruleset"]["rules"][0]["id"], "reminder");
    assert_eq!(updated.result["event"]["kind"], "group.automation_update");
    assert_eq!(
        updated.result["supported_vars"],
        json!([
            "interval_minutes",
            "group_title",
            "actor_names",
            "scheduled_at"
        ])
    );
    assert!(
        updated.result["status"]["reminder"]["next_fire_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "every configured rule must have UI timing status before its first tick"
    );

    let stale = raw(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "expected_version":1,
            "ruleset":{"rules":[],"snippets":{}}
        }),
    );
    assert!(!stale.ok);
    assert_eq!(stale.error.expect("error").code, "version_conflict");
}

#[test]
fn automation_manage_applies_batched_actions_and_reset_honors_version() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"automation manage"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");

    let managed = ok(
        &home,
        "group_automation_manage",
        json!({
            "group_id":group_id,
            "expected_version":1,
            "actions":[
                {"type":"create_rule","rule":{
                    "id":"personal","enabled":false,"scope":"personal",
                    "owner_actor_id":"user","to":["user"],
                    "trigger":{"kind":"interval","every_seconds":60},
                    "action":{"kind":"notify","message":"check"}
                }},
                {"type":"set_rule_enabled","rule_id":"personal","enabled":true}
            ],
            "by":"user"
        }),
    );
    assert_eq!(managed.result["changed"], true);
    assert_eq!(
        managed.result["applied_actions"]
            .as_array()
            .expect("applied_actions must be an array")
            .len(),
        2
    );
    assert_eq!(managed.result["version"], 2);
    assert_eq!(
        managed.result["ruleset"]["rules"]
            .as_array()
            .expect("ruleset rules must be an array")
            .iter()
            .find(|rule| rule["id"] == "personal")
            .expect("personal rule must exist")["enabled"],
        true
    );

    let stale = raw(
        &home,
        "group_automation_reset_baseline",
        json!({"group_id":group_id,"expected_version":1,"by":"user"}),
    );
    assert_eq!(stale.error.expect("stale error").code, "version_conflict");
    let reset = ok(
        &home,
        "group_automation_reset_baseline",
        json!({"group_id":group_id,"expected_version":2,"by":"user"}),
    );
    assert_eq!(reset.result["version"], 3);
    assert_eq!(reset.result["ruleset"]["rules"][0]["id"], "standup");
    assert_eq!(reset.result["event"]["kind"], "group.automation_update");
}

#[test]
fn changing_a_one_time_rule_retires_its_previous_runtime_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(
        &home,
        "group_create",
        json!({"title":"automation generation"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    ok(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","runtime":"custom","by":"user"}),
    );
    let rule = |at: &str| {
        json!({
            "id":"once","enabled":true,"scope":"group","to":["@all"],
            "trigger":{"kind":"at","at":at},
            "action":{"kind":"notify","message":"run once"}
        })
    };
    ok(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "expected_version":1,
            "ruleset":{"rules":[rule("2020-01-01T00:00:00Z")],"snippets":{}}
        }),
    );
    assert_eq!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("first tick")
            .notifications
            .len(),
        1
    );

    ok(
        &home,
        "group_automation_manage",
        json!({
            "group_id":group_id,"expected_version":2,"by":"user",
            "actions":[{"type":"update_rule","rule":rule("2020-01-02T00:00:00Z")}]
        }),
    );

    assert_eq!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("updated tick")
            .notifications
            .len(),
        1,
        "the updated one-time rule is a new schedule generation"
    );
}

#[test]
fn ruleset_change_rolls_back_when_runtime_reconcile_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(
        &home,
        "group_create",
        json!({"title":"automation rollback"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let rule = |at: &str| {
        json!({
            "id":"once","enabled":true,"scope":"group","to":["@all"],
            "trigger":{"kind":"at","at":at},
            "action":{"kind":"notify","message":"run once"}
        })
    };
    ok(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "expected_version":1,
            "ruleset":{"rules":[rule("2030-01-01T00:00:00Z")],"snippets":{}}
        }),
    );
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let state_path = store
        .state_dir(group_id)
        .expect("state dir")
        .join("automation.json");
    std::fs::write(&state_path, b"not json").expect("corrupt state fixture");

    let failed = raw(
        &home,
        "group_automation_manage",
        json!({
            "group_id":group_id,"expected_version":2,"by":"user",
            "actions":[{"type":"update_rule","rule":rule("2030-01-02T00:00:00Z")}]
        }),
    );

    assert!(!failed.ok);
    let reloaded = store.load(group_id).expect("rolled-back group");
    assert_eq!(
        reloaded.automation["rules"][0]["trigger"]["at"],
        "2030-01-01T00:00:00Z"
    );
}

#[test]
fn resume_suppresses_missed_one_time_work_without_retiring_future_work() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"automation resume"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let rule = |id: &str, at: &str| {
        json!({
            "id":id,"enabled":true,"scope":"group","to":["@all"],
            "trigger":{"kind":"at","at":at},
            "action":{"kind":"notify","message":id}
        })
    };
    ok(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "expected_version":1,
            "ruleset":{
                "rules":[
                    rule("missed", "2020-01-01T00:00:00Z"),
                    rule("future", "2099-01-01T00:00:00Z")
                ],
                "snippets":{}
            }
        }),
    );
    ok(
        &home,
        "group_set_state",
        json!({"group_id":group_id,"state":"paused","by":"user"}),
    );
    assert!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("paused tick")
            .notifications
            .is_empty()
    );
    ok(
        &home,
        "group_set_state",
        json!({"group_id":group_id,"state":"active","by":"user"}),
    );

    assert!(
        cccc_core::automation::tick_group(&home, group_id, false)
            .expect("resumed tick")
            .notifications
            .is_empty(),
        "resume must not catch up stale one-time work"
    );
    let state: Value = cccc_core::fs::read_json(
        &cccc_core::GroupStore::new(home)
            .expect("store")
            .state_dir(group_id)
            .expect("state dir")
            .join("automation.json"),
    )
    .expect("automation state");
    assert!(state["rules"]["missed"]["last_fired_at"].is_string());
    assert!(state["rules"]["future"].get("last_fired_at").is_none());
}

#[test]
fn automation_state_filters_other_peers_personal_rules_and_snippets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"automation privacy"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for actor_id in ["lead", "peer1", "peer2"] {
        ok(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":actor_id,
                "role":"peer",
                "runtime":"custom",
                "by":"user"
            }),
        );
    }
    let rule = |id: &str, scope: &str, owner: Option<&str>, snippet_ref: &str| {
        json!({
            "id":id,"enabled":true,"scope":scope,"owner_actor_id":owner,
            "to":[owner.unwrap_or("@all")],
            "trigger":{"kind":"interval","every_seconds":60},
            "action":{"kind":"notify","snippet_ref":snippet_ref}
        })
    };
    ok(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "expected_version":1,
            "ruleset":{
                "rules":[
                    rule("group-rule", "group", None, "group-text"),
                    rule("peer1-rule", "personal", Some("peer1"), "peer1-text"),
                    rule("peer2-rule", "personal", Some("peer2"), "peer2-text")
                ],
                "snippets":{
                    "group-text":"group",
                    "peer1-text":"peer one",
                    "peer2-text":"peer two"
                }
            }
        }),
    );
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    cccc_core::fs::write_json(
        &store
            .state_dir(group_id)
            .expect("state dir")
            .join("automation.json"),
        &json!({
            "v":5,
            "rules":{
                "group-rule":{"last_fired_at":"2026-08-11T00:00:00Z"},
                "peer1-rule":{"last_error_at":"2026-08-11T00:01:00Z","last_error":"delivery failed"},
                "peer2-rule":{"last_error_at":"2026-08-11T00:02:00Z","last_error":"private failure"}
            }
        }),
    )
    .expect("runtime state");

    let state = ok(
        &home,
        "group_automation_state",
        json!({"group_id":group_id,"by":"peer1"}),
    );
    let ids = state.result["ruleset"]["rules"]
        .as_array()
        .expect("visible rules")
        .iter()
        .filter_map(|rule| rule["id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["group-rule", "peer1-rule"]);
    assert_eq!(
        state.result["ruleset"]["snippets"],
        json!({"group-text":"group","peer1-text":"peer one"})
    );
    assert!(
        state.result["snippet_catalog"]["custom"]
            .get("peer2-text")
            .is_none()
    );
    assert_eq!(
        state.result["status"]["peer1-rule"]["last_error"],
        "delivery failed"
    );
    assert!(state.result["status"].get("peer2-rule").is_none());

    let unknown = raw(
        &home,
        "group_automation_state",
        json!({"group_id":group_id,"by":"missing"}),
    );
    assert_eq!(
        unknown.error.expect("unknown actor error").code,
        "permission_denied"
    );
}

#[test]
fn new_interval_rules_wait_before_first_delivery_and_render_supported_vars() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"Automation Team"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    ok(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"peer1","title":"Peer One",
            "role":"peer","runtime":"custom","by":"user"
        }),
    );
    ok(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "ruleset":{
                "rules":[{
                    "id":"interval","enabled":true,"scope":"group","to":["peer1"],
                    "trigger":{"kind":"interval","every_seconds":60},
                    "action":{"kind":"notify","message":"{{ interval_minutes }}m {{group_title}} / {{actor_names}} / {{scheduled_at}}"}
                }],
                "snippets":{}
            }
        }),
    );

    let first = cccc_core::automation::tick_group(&home, group_id, false).expect("first tick");
    assert!(
        first.notifications.is_empty(),
        "a new interval starts its clock instead of firing immediately"
    );

    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let state_path = store
        .state_dir(group_id)
        .expect("state dir")
        .join("automation.json");
    let mut runtime: Value = cccc_core::fs::read_json(&state_path).expect("runtime state");
    runtime["rules"]["interval"]["last_fired_at"] = json!("2020-01-01T00:00:00Z");
    cccc_core::fs::write_json(&state_path, &runtime).expect("due runtime state");

    let due = cccc_core::automation::tick_group(&home, group_id, false).expect("due tick");
    let text = due.notifications[0].data["message"]
        .as_str()
        .expect("rendered text");
    assert!(
        text.starts_with("1m Automation Team / Peer One / "),
        "{text}"
    );
    assert!(!text.contains("{{"), "{text}");
}

#[test]
fn automation_writes_reject_malformed_cross_engine_state() {
    let invalid_rulesets = [
        json!({"rules":"not-an-array","snippets":{}}),
        json!({
            "rules":[
                {"id":"duplicate","trigger":{"kind":"interval","every_seconds":60}},
                {"id":"duplicate","trigger":{"kind":"interval","every_seconds":60}}
            ],
            "snippets":{}
        }),
        json!({
            "rules":[{
                "id":"legacy","schedule":{"kind":"interval","every_seconds":60},
                "message":"legacy root message",
                "trigger":{"kind":"interval","every_seconds":60}
            }],
            "snippets":{}
        }),
        json!({
            "rules":[{
                "id":"unsafe-repeat","trigger":{"kind":"interval","every_seconds":60},
                "action":{"kind":"group_state","state":"stopped"}
            }],
            "snippets":{}
        }),
        json!({
            "rules":[{"id":"valid","trigger":{"kind":"interval","every_seconds":60}}],
            "snippets":{"bad":7}
        }),
    ];
    for (index, ruleset) in invalid_rulesets.into_iter().enumerate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let created = ok(
            &home,
            "group_create",
            json!({"title":format!("invalid automation {index}")}),
        );
        let group_id = created.result["group"]["group_id"]
            .as_str()
            .expect("group id");
        let response = raw(
            &home,
            "group_automation_update",
            json!({"group_id":group_id,"ruleset":ruleset}),
        );
        assert!(!response.ok, "invalid ruleset {index} was persisted");
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"invalid manage"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let response = raw(
        &home,
        "group_automation_manage",
        json!({
            "group_id":group_id,
            "actions":[{"type":"create_rule","rule":{
                "id":"unsafe-repeat","trigger":{"kind":"interval","every_seconds":60},
                "action":{"kind":"actor_control","operation":"restart","targets":["@all"]}
            }}]
        }),
    );
    assert!(
        !response.ok,
        "incremental writes must enforce the same action/trigger contract"
    );

    let invalid_version = raw(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "expected_version":"not-an-int",
            "ruleset":{"rules":[],"snippets":{}}
        }),
    );
    assert!(
        !invalid_version.ok,
        "expected_version must not be silently ignored"
    );
}

#[test]
fn cron_writes_reject_new_bad_expressions_but_keep_unchanged_legacy_rules_saveable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"cron legacy"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let cron_rule = |id: &str, cron: &str, message: &str| {
        json!({
            "id":id,"enabled":false,"to":["@all"],
            "trigger":{"kind":"cron","cron":cron},
            "action":{"kind":"notify","message":message}
        })
    };
    // A rule saved before expressions were validated, which can no longer be written fresh.
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.load(group_id).expect("load group");
    group.automation.insert(
        "rules".into(),
        json!([cron_rule("legacy", "not a cron", "old")]),
    );
    store.save(&group).expect("seed legacy rule");

    // Saving the ruleset with the legacy rule untouched and another rule edited succeeds.
    let saved = raw(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "ruleset":{
                "rules":[
                    cron_rule("legacy", "not a cron", "old"),
                    cron_rule("weekly", "0 9 * * 1", "monday")
                ],
                "snippets":{}
            }
        }),
    );
    assert!(
        saved.ok,
        "unchanged legacy rule blocked the save: {:?}",
        saved.error
    );
    let replaced = raw(
        &home,
        "group_automation_manage",
        json!({
            "group_id":group_id,
            "actions":[{"type":"replace_all_rules","ruleset":{
                "rules":[cron_rule("legacy", "not a cron", "still old")],
                "snippets":{}
            }}]
        }),
    );
    assert!(
        replaced.ok,
        "replace_all_rules rejected the unchanged legacy rule: {:?}",
        replaced.error
    );

    // Writing a bad expression, new or changed, is still rejected on every write path.
    for args in [
        json!({"group_id":group_id,"ruleset":{"rules":[cron_rule("legacy", "still not a cron", "old")],"snippets":{}}}),
        json!({"group_id":group_id,"ruleset":{"rules":[cron_rule("fresh", "not a cron", "new")],"snippets":{}}}),
    ] {
        let response = raw(&home, "group_automation_update", args);
        assert_cron_rejected(response);
    }
    let changed = raw(
        &home,
        "group_automation_manage",
        json!({
            "group_id":group_id,
            "actions":[{"type":"update_rule","rule":cron_rule("legacy", "0 0 * * 8", "old")}]
        }),
    );
    assert_cron_rejected(changed);
}

/// A Group whose first Actor `lead` is the foreman, with peers `peer1` and
/// `peer2`, and user-made rules: two scheduled operations, a team reminder and
/// a personal reminder of `peer2`.
fn group_with_user_rules(home: &HomeLayout) -> String {
    let created = ok(home, "group_create", json!({"title":"automation access"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    for actor_id in ["lead", "peer1", "peer2"] {
        ok(
            home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"runtime":"custom","by":"user"}),
        );
    }
    ok(
        home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "ruleset":{
                "rules":[
                    operation("nightly-pause", json!({"kind":"group_state","state":"paused"})),
                    operation("morning-restart", json!({"kind":"actor_control","operation":"restart","targets":["@all"]})),
                    {
                        "id":"team","enabled":true,"to":["@all"],
                        "trigger":{"kind":"interval","every_seconds":3600},
                        "action":{"kind":"notify","snippet_ref":"team-text"}
                    },
                    reminder("peer2-own", "peer2")
                ],
                "snippets":{"team-text":"sync up"}
            },
            "by":"user"
        }),
    );
    group_id
}

fn operation(id: &str, action: Value) -> Value {
    json!({
        "id":id,"enabled":true,
        "trigger":{"kind":"at","at":"2030-01-01T00:00:00Z"},
        "action":action
    })
}

fn reminder(id: &str, owner: &str) -> Value {
    json!({
        "id":id,"enabled":true,"scope":"personal","owner_actor_id":owner,"to":[owner],
        "trigger":{"kind":"interval","every_seconds":600},
        "action":{"kind":"notify","message":"check CI"}
    })
}

fn stored_automation(home: &HomeLayout, group_id: &str) -> Value {
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    Value::Object(store.load(group_id).expect("load group").automation)
}

fn stored_ids(home: &HomeLayout, group_id: &str) -> Vec<String> {
    stored_automation(home, group_id)["rules"]
        .as_array()
        .expect("saved rules")
        .iter()
        .filter_map(|rule| rule["id"].as_str().map(str::to_owned))
        .collect()
}

fn manage_as(home: &HomeLayout, group_id: &str, by: &str, actions: Value) -> DaemonResponse {
    raw(
        home,
        "group_automation_manage",
        json!({"group_id":group_id,"by":by,"actions":actions}),
    )
}

fn denied(response: DaemonResponse) -> String {
    assert!(!response.ok, "an unauthorized automation change was saved");
    let error = response.error.expect("error");
    assert_eq!(error.code, "permission_denied", "{}", error.message);
    error.message
}

#[test]
fn a_peer_manages_its_own_reminders_and_nothing_else() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = group_with_user_rules(&home);

    // Fields a peer leaves out mean a reminder to itself.
    let created = manage_as(
        &home,
        &group_id,
        "peer1",
        json!([{"type":"create_rule","rule":{
            "id":"mine","trigger":{"kind":"interval","every_seconds":300},
            "action":{"kind":"notify","message":"look at the build"}
        }}]),
    );
    assert!(created.ok, "{:?}", created.error);
    let mine = stored_automation(&home, &group_id)["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .find(|rule| rule["id"] == "mine")
        .cloned()
        .expect("saved reminder");
    assert_eq!(mine["scope"], "personal");
    assert_eq!(mine["owner_actor_id"], "peer1");
    assert_eq!(mine["to"], json!(["peer1"]));
    for actions in [
        json!([{"type":"set_rule_enabled","rule_id":"mine","enabled":false}]),
        json!([{"type":"update_rule","rule":reminder("mine", "peer1")}]),
        json!([{"type":"delete_rule","rule_id":"mine"}]),
    ] {
        let response = manage_as(&home, &group_id, "peer1", actions);
        assert!(response.ok, "{:?}", response.error);
    }

    let saved = stored_automation(&home, &group_id);
    let mut group_rule = reminder("shared", "peer1");
    group_rule["scope"] = json!("group");
    let mut to_everyone = reminder("loud", "peer1");
    to_everyone["to"] = json!(["@all"]);
    for rule in [group_rule, to_everyone, reminder("forged", "peer2")] {
        denied(manage_as(
            &home,
            &group_id,
            "peer1",
            json!([{"type":"create_rule","rule":rule}]),
        ));
    }
    let message = denied(manage_as(
        &home,
        &group_id,
        "peer1",
        json!([{"type":"set_rule_enabled","rule_id":"team","enabled":false}]),
    ));
    assert!(message.contains("team"), "{message}");

    // Another peer's reminder is neither changed nor named to this peer.
    let message = denied(manage_as(
        &home,
        &group_id,
        "peer1",
        json!([{"type":"delete_rule","rule_id":"peer2-own"}]),
    ));
    assert!(!message.contains("peer2-own"), "{message}");
    assert!(message.contains("1 rule not visible to you"), "{message}");

    // Writing back its filtered view would drop the rules it cannot see.
    let view = ok(
        &home,
        "group_automation_state",
        json!({"group_id":group_id,"by":"peer1"}),
    );
    let message = denied(manage_as(
        &home,
        &group_id,
        "peer1",
        json!([{"type":"replace_all_rules","ruleset":view.result["ruleset"]}]),
    ));
    assert!(!message.contains("peer2-own"), "{message}");
    denied(raw(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"by":"peer1","ruleset":view.result["ruleset"]}),
    ));
    denied(raw(
        &home,
        "group_automation_reset_baseline",
        json!({"group_id":group_id,"by":"peer1"}),
    ));
    assert_eq!(stored_automation(&home, &group_id), saved);
}

#[test]
fn a_foreman_manages_reminders_but_not_scheduled_operations() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = group_with_user_rules(&home);

    let team_reminder = json!({
        "id":"standup-now","to":["@peers"],
        "trigger":{"kind":"interval","every_seconds":1800},
        "action":{"kind":"notify","message":"post status"}
    });
    for actions in [
        json!([{"type":"create_rule","rule":team_reminder}]),
        json!([{"type":"set_rule_enabled","rule_id":"peer2-own","enabled":false}]),
    ] {
        let response = manage_as(&home, &group_id, "lead", actions);
        assert!(response.ok, "{:?}", response.error);
    }

    let saved = stored_automation(&home, &group_id);
    let mut converted = operation("nightly-pause", Value::Null);
    converted["trigger"] = json!({"kind":"interval","every_seconds":60});
    converted["action"] = json!({"kind":"notify","message":"no longer pauses"});
    for actions in [
        json!([{"type":"set_rule_enabled","rule_id":"nightly-pause","enabled":false}]),
        json!([{"type":"delete_rule","rule_id":"morning-restart"}]),
        json!([{"type":"update_rule","rule":converted}]),
        json!([{"type":"create_rule","rule":operation("late-stop", json!({"kind":"group_state","state":"stopped"}))}]),
        // One refused action refuses the whole batch.
        json!([
            {"type":"create_rule","rule":reminder("lead-own", "lead")},
            {"type":"delete_rule","rule_id":"nightly-pause"}
        ]),
    ] {
        let message = denied(manage_as(&home, &group_id, "lead", actions));
        assert!(message.contains("only the user"), "{message}");
    }
    denied(raw(
        &home,
        "group_automation_reset_baseline",
        json!({"group_id":group_id,"by":"lead"}),
    ));
    assert_eq!(stored_automation(&home, &group_id), saved);

    // Bulk writes may keep the user's operations as they are, but not reorder them.
    let mut rules = saved["rules"].as_array().expect("rules").clone();
    rules.retain(|rule| rule["id"] != "team");
    let kept = raw(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"by":"lead","ruleset":{"rules":rules,"snippets":{"team-text":"sync up"}}}),
    );
    assert!(kept.ok, "{:?}", kept.error);
    let mut swapped = rules.clone();
    swapped.swap(0, 1);
    let message = denied(manage_as(
        &home,
        &group_id,
        "lead",
        json!([{"type":"replace_all_rules","ruleset":{"rules":swapped,"snippets":{}}}]),
    ));
    assert!(message.contains("reorders"), "{message}");
    let operations_only = rules[..2].to_vec();
    let replaced = manage_as(
        &home,
        &group_id,
        "lead",
        json!([{"type":"replace_all_rules","ruleset":{"rules":operations_only,"snippets":{}}}]),
    );
    assert!(replaced.ok, "{:?}", replaced.error);
    assert_eq!(
        stored_ids(&home, &group_id),
        ["nightly-pause", "morning-restart"]
    );
}

#[test]
fn the_baseline_survives_a_first_change_and_a_saved_empty_list_stays_empty() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"baseline"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    ok(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"lead","runtime":"custom","by":"user"}),
    );
    ok(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","runtime":"custom","by":"user"}),
    );
    let created = manage_as(
        &home,
        group_id,
        "peer1",
        json!([{"type":"create_rule","rule":reminder("mine", "peer1")}]),
    );
    assert!(created.ok, "{:?}", created.error);
    assert_eq!(stored_ids(&home, group_id), ["standup", "mine"]);

    ok(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"ruleset":{"rules":[],"snippets":{}},"by":"user"}),
    );
    let state = ok(
        &home,
        "group_automation_state",
        json!({"group_id":group_id}),
    );
    assert_eq!(state.result["ruleset"]["rules"], json!([]));
    let created = manage_as(
        &home,
        group_id,
        "lead",
        json!([{"type":"create_rule","rule":reminder("lead-own", "lead")}]),
    );
    assert!(created.ok, "{:?}", created.error);
    assert_eq!(stored_ids(&home, group_id), ["lead-own"]);

    denied(manage_as(
        &home,
        group_id,
        "stranger",
        json!([{"type":"delete_rule","rule_id":"lead-own"}]),
    ));
}

#[test]
fn replacing_the_list_saves_snippets_the_way_they_take_effect() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"snippets"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for actor_id in ["lead", "peer1"] {
        ok(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"runtime":"custom","by":"user"}),
        );
    }
    let created = manage_as(
        &home,
        group_id,
        "peer1",
        json!([{"type":"create_rule","rule":reminder("mine", "peer1")}]),
    );
    assert!(created.ok, "{:?}", created.error);

    // A peer writing its view back unchanged, built-in stand-up text included,
    // changes nothing and is not refused for it.
    let view = ok(
        &home,
        "group_automation_state",
        json!({"group_id":group_id,"by":"peer1"}),
    );
    assert!(view.result["ruleset"]["snippets"]["standup"].is_string());
    let unchanged = manage_as(
        &home,
        group_id,
        "peer1",
        json!([{"type":"replace_all_rules","ruleset":view.result["ruleset"]}]),
    );
    assert!(unchanged.ok, "{:?}", unchanged.error);
    assert_eq!(unchanged.result["changed"], false);

    // A changed stand-up text replaces the saved override, so the text that
    // fires is the one the foreman saved.
    ok(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"by":"user","ruleset":{
            "rules":view.result["ruleset"]["rules"],"snippets":{"standup":"old text"}
        }}),
    );
    let replaced = manage_as(
        &home,
        group_id,
        "lead",
        json!([{"type":"replace_all_rules","ruleset":{
            "rules":view.result["ruleset"]["rules"],"snippets":{"standup":"new text"}
        }}]),
    );
    assert!(replaced.ok, "{:?}", replaced.error);
    let saved = stored_automation(&home, group_id);
    assert_eq!(saved["snippet_overrides"], json!({"standup":"new text"}));
    assert_eq!(saved["snippets"], json!({}));
    let state = ok(
        &home,
        "group_automation_state",
        json!({"group_id":group_id}),
    );
    assert_eq!(state.result["ruleset"]["snippets"]["standup"], "new text");
}

#[test]
fn agent_writes_report_a_malformed_saved_list_instead_of_replacing_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"malformed"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    ok(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"lead","runtime":"custom","by":"user"}),
    );
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.load(group_id).expect("load group");
    group.automation.insert("rules".into(), Value::Null);
    store.save(&group).expect("seed malformed rules");

    let response = manage_as(
        &home,
        group_id,
        "lead",
        json!([{"type":"create_rule","rule":reminder("lead-own", "lead")}]),
    );
    assert_eq!(
        response.error.expect("refused").code,
        "group_automation_state_invalid"
    );
    assert_eq!(stored_automation(&home, group_id)["rules"], Value::Null);
    ok(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"by":"user","ruleset":{"rules":[],"snippets":{}}}),
    );
    assert_eq!(stored_automation(&home, group_id)["rules"], json!([]));
}

#[test]
fn enabling_or_disabling_a_rule_takes_an_explicit_boolean() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = group_with_user_rules(&home);
    let before = stored_automation(&home, &group_id);
    for enabled in [None, Some(Value::Null), Some(json!("true")), Some(json!(1))] {
        let mut action = json!({"type":"set_rule_enabled","rule_id":"team"});
        if let Some(enabled) = enabled {
            action["enabled"] = enabled;
        }
        let response = manage_as(&home, &group_id, "user", json!([action]));
        assert_eq!(response.error.expect("refused").code, "invalid_request");
    }
    assert_eq!(stored_automation(&home, &group_id), before);
}

#[test]
fn an_actor_caller_is_the_actor_named_exactly() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group_id = group_with_user_rules(&home);
    // A padded name would be authorized as peer2 but own its rules as another.
    let padded = manage_as(
        &home,
        &group_id,
        " peer2",
        json!([{"type":"create_rule","rule":reminder("padded", " peer2")}]),
    );
    assert!(denied(padded).contains("unknown actor"));
}

#[test]
fn a_change_keeps_every_saved_rule_or_saves_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = ok(&home, "group_create", json!({"title":"rule ids"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let rule = |id: &str| {
        json!({
            "id":id,"trigger":{"kind":"interval","every_seconds":600},
            "action":{"kind":"notify","message":"check CI"}
        })
    };
    // Ids are compared as written, as the scheduler keys their state, so
    // these are two rules and a change to one keeps the other.
    ok(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"ruleset":{"rules":[rule("same"), rule(" same ")]}}),
    );
    let create = json!([{"type":"create_rule","rule":rule("other")}]);
    assert!(manage_as(&home, group_id, "user", create).ok);
    assert_eq!(stored_ids(&home, group_id), ["same", " same ", "other"]);
    let disable = json!([{"type":"set_rule_enabled","rule_id":" same ","enabled":false}]);
    assert!(manage_as(&home, group_id, "user", disable).ok);
    let rules = stored_automation(&home, group_id)["rules"].clone();
    assert_eq!(
        (&rules[0]["enabled"], &rules[1]["enabled"]),
        (&json!(true), &json!(false))
    );
    let delete = json!([{"type":"delete_rule","rule_id":"same"}]);
    assert!(manage_as(&home, group_id, "user", delete).ok);
    assert_eq!(stored_ids(&home, group_id), [" same ", "other"]);

    // A saved rule that a change could not carry over is reported, not dropped.
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    for rules in [
        json!([rule("kept"), {"trigger":{"kind":"interval","every_seconds":600}}]),
        json!([rule("kept"), "not a rule"]),
        json!([rule("kept"), rule("  ")]),
        json!([rule("twin"), rule("twin")]),
    ] {
        let mut group = store.load(group_id).expect("load group");
        group.automation.insert("rules".into(), rules.clone());
        store.save(&group).expect("seed saved rules");
        let create = json!([{"type":"create_rule","rule":rule("new")}]);
        let response = manage_as(&home, group_id, "user", create);
        assert_eq!(
            response.error.expect("refused").code,
            "group_automation_state_invalid"
        );
        assert_eq!(stored_automation(&home, group_id)["rules"], rules);
    }
    ok(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"ruleset":{"rules":[rule("twin")]}}),
    );
    assert_eq!(stored_ids(&home, group_id), ["twin"]);
}

fn assert_cron_rejected(response: DaemonResponse) {
    assert!(!response.ok, "a bad cron expression was persisted");
    let message = response.error.expect("rejected").message;
    assert!(message.contains("invalid cron expression"), "{message}");
}

fn ok(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw(home, op, args);
    assert!(response.ok, "{op} failed: {:?}", response.error);
    response
}

fn raw(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
