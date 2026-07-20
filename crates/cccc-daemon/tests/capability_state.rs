use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn legacy_registered_skills_are_projected_for_slash_commands() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    std::fs::create_dir_all(home.root().join("state/capabilities")).expect("capability state");
    write_json(
        &home.root().join("state/capabilities/catalog.json"),
        json!({"records":{"skill:test:review":{
            "capability_id":"skill:test:review","kind":"skill","name":"review",
            "description_short":"Review code","capsule_text":"Skill: review",
            "source_id":"github_skills_curated",
            "source_uri":"https://example.test/review"
        }}}),
    );
    write_json(
        &home.root().join("state/capabilities/state.json"),
        json!({
            "group_enabled":{"g_test":["skill:test:review"]},
            "actor_hidden":{"g_test":{"other":["skill:test:review"]}}
        }),
    );

    let result = call(
        &home,
        "capability_state",
        json!({"group_id":"g_test","actor_id":"user","view":"slash_commands"}),
    );

    assert_eq!(result["group_id"], "g_test");
    assert_eq!(result["actor_id"], "user");
    assert_eq!(result["enabled_capabilities"], json!(["skill:test:review"]));
    assert_eq!(
        result["active_capsule_skills"][0]["capability_id"],
        "skill:test:review"
    );
    assert_eq!(result["active_capsule_skills"][0]["name"], "review");
    assert_eq!(
        result["active_capsule_skills"][0]["source_uri"],
        "https://example.test/review"
    );

    let overview = call(
        &home,
        "capability_overview",
        json!({"kind":"skill","limit":80,"offset":0}),
    );
    assert_eq!(overview["total_count"], 1);
    assert_eq!(overview["kind_counts"]["skill"], 1);
    assert_eq!(overview["items"][0]["capability_id"], "skill:test:review");
    assert_eq!(overview["items"][0]["kind"], "skill");
    assert_eq!(overview["items"][0]["source_id"], "github_skills_curated");
    assert_eq!(
        overview["items"][0]["source_uri"],
        "https://example.test/review"
    );
}

#[test]
fn native_updates_override_legacy_capability_flags() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    std::fs::create_dir_all(home.root().join("state/capabilities")).expect("capability state");
    write_json(
        &home.root().join("state/capabilities/catalog.json"),
        json!({"records":{
            "skill:test:enabled":capability("skill:test:enabled"),
            "skill:test:blocked":capability("skill:test:blocked"),
            "skill:test:hidden":capability("skill:test:hidden")
        }}),
    );
    write_json(
        &home.root().join("state/capabilities/state.json"),
        json!({
            "group_enabled":{"g_test":["skill:test:enabled"]},
            "global_blocked":["skill:test:blocked"],
            "actor_hidden":{"g_test":{"user":["skill:test:hidden"]}}
        }),
    );

    let blocked = call(
        &home,
        "capability_overview",
        json!({"group_id":"g_test","policy":"blocked"}),
    );
    assert_eq!(blocked["total_count"], 1);
    assert_eq!(blocked["items"][0]["capability_id"], "skill:test:blocked");
    assert_eq!(blocked["items"][0]["qualification_status"], "blocked");

    call(
        &home,
        "capability_enable",
        json!({"capability_id":"skill:test:enabled","enabled":false}),
    );
    call(
        &home,
        "capability_block",
        json!({"capability_id":"skill:test:blocked","blocked":false}),
    );
    call(
        &home,
        "capability_visibility",
        json!({"capability_id":"skill:test:hidden","hidden":false}),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":"g_test","actor_id":"user"}),
    );
    assert_eq!(state["enabled_capabilities"], json!([]));
    assert_eq!(state["actor_hidden_capabilities"], json!([]));
    assert_eq!(state["state"]["disabled"], json!(["skill:test:enabled"]));
    assert_eq!(state["state"]["unblocked"], json!(["skill:test:blocked"]));
    assert_eq!(state["state"]["visible"], json!(["skill:test:hidden"]));

    let blocked = call(
        &home,
        "capability_overview",
        json!({"group_id":"g_test","policy":"blocked"}),
    );
    assert_eq!(blocked["total_count"], 0);
    assert_eq!(blocked["blocked_capabilities"], json!([]));
}

fn capability(id: &str) -> Value {
    json!({
        "capability_id":id,
        "kind":"skill",
        "name":id,
        "description_short":"test capability"
    })
}

fn call(home: &HomeLayout, op: &str, args: Value) -> Map<String, Value> {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response.result
}

fn write_json(path: &std::path::Path, value: Value) {
    std::fs::write(path, serde_json::to_vec(&value).expect("json")).expect("fixture");
}
