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
