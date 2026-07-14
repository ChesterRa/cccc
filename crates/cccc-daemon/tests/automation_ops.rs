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
    assert_eq!(initial.result["version"], 1);

    let updated = ok(
        &home,
        "group_automation_update",
        json!({
            "group_id":group_id,
            "patch":{
                "expected_version":1,
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

    let stale = raw(
        &home,
        "group_automation_update",
        json!({"group_id":group_id,"patch":{"expected_version":1,"rules":[]}}),
    );
    assert!(!stale.ok);
    assert_eq!(stale.error.expect("error").code, "version_conflict");
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
