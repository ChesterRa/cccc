// Included by the crate-level integration test harness.
use cccc_contracts::DaemonRequest;
use cccc_core::GroupStore;
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
        json!({
            "group_id":"g_test",
            "actor_id":"user",
            "scope":"group",
            "capability_id":"skill:test:enabled",
            "enabled":false
        }),
    );
    call(
        &home,
        "capability_block",
        json!({
            "group_id":"g_test",
            "actor_id":"user",
            "scope":"global",
            "capability_id":"skill:test:blocked",
            "blocked":false
        }),
    );
    call(
        &home,
        "capability_visibility",
        json!({
            "group_id":"g_test",
            "actor_id":"user",
            "capability_id":"skill:test:hidden",
            "hidden":false
        }),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":"g_test","actor_id":"user"}),
    );
    assert_eq!(state["enabled_capabilities"], json!([]));
    assert_eq!(state["actor_hidden_capabilities"], json!([]));
    let stored: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert!(stored["group_enabled"].get("g_test").is_none());
    assert!(
        stored["global_blocked"]
            .as_object()
            .expect("global")
            .is_empty()
    );
    assert!(stored["actor_hidden"].get("g_test").is_none());

    let blocked = call(
        &home,
        "capability_overview",
        json!({"group_id":"g_test","policy":"blocked"}),
    );
    assert_eq!(blocked["total_count"], 0);
    assert_eq!(blocked["blocked_capabilities"], json!([]));
}

#[test]
fn local_skill_install_and_uninstall_complete_the_lifecycle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability lifecycle", "")
        .expect("group");
    let skill_dir = temp.path().join("review");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: review\ndescription: Review changes\n---\nReview carefully.\n",
    )
    .expect("skill");

    let installed = call(
        &home,
        "capability_install_target",
        json!({"group_id":group.group_id,"target":skill_dir,"scope":"group","by":"user"}),
    );
    assert_eq!(installed["state"], "ready");
    assert_eq!(
        installed["installed_capability_ids"][0],
        "skill:local:review"
    );
    let record: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/catalog.json")).expect("catalog"),
    )
    .expect("catalog JSON");
    assert_eq!(
        record["records"]["skill:local:review"]["source_id"],
        "local_import"
    );

    let removed = call(
        &home,
        "capability_uninstall",
        json!({"group_id":group.group_id,"capability_id":"skill:local:review","by":"user"}),
    );
    assert_eq!(removed["removed_record"], true);
    assert!(removed["removed_bindings"].as_u64().unwrap_or(0) > 0);
    let record: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/catalog.json")).expect("catalog"),
    )
    .expect("catalog JSON");
    assert!(record["records"].get("skill:local:review").is_none());
}

#[test]
fn capability_import_dry_run_and_invalid_install_do_not_persist() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability safety", "")
        .expect("group");

    let dry_run = response(
        &home,
        "capability_import",
        json!({
            "group_id":group.group_id,"by":"user","dry_run":true,
            "record":{"capability_id":"skill:test:dry","kind":"skill","name":"dry","capsule_text":"test"}
        }),
    );
    assert!(dry_run.ok, "{:?}", dry_run.error);
    assert_eq!(dry_run.result["imported"], false);
    assert!(!home.root().join("state/capabilities/catalog.json").exists());

    let skill_dir = temp.path().join("invalid-scope");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: invalid-scope\ndescription: Test invalid scope\n---\nTest.\n",
    )
    .expect("skill");
    let install = response(
        &home,
        "capability_install_target",
        json!({"group_id":group.group_id,"target":skill_dir,"scope":"invalid","by":"user"}),
    );
    assert!(!install.ok);
    assert!(!home.root().join("state/capabilities/catalog.json").exists());

    let store = cccc_core::capabilities::CapabilityStore::new(home.clone());
    store
        .import_record(json!({
            "capability_id":"skill:test:existing","kind":"skill","name":"Original"
        }))
        .expect("existing record");
    let overwrite = response(
        &home,
        "capability_import",
        json!({
            "group_id":group.group_id,"by":"user","dry_run":true,
            "record":{"capability_id":"skill:test:existing","kind":"skill","name":"Changed"}
        }),
    );
    assert!(overwrite.ok, "{:?}", overwrite.error);
    assert_eq!(
        store
            .catalog_record("skill:test:existing")
            .expect("catalog")
            .expect("record")["name"],
        "Original"
    );
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
    let response = response(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response.result
}

fn response(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        },
    )
}

fn write_json(path: &std::path::Path, value: Value) {
    std::fs::write(path, serde_json::to_vec(&value).expect("json")).expect("fixture");
}
