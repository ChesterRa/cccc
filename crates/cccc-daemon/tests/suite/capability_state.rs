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
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("native capability state", "")
        .expect("group");
    let group_id = group.group_id;
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
            "group_enabled":{(group_id.clone()):["skill:test:enabled"]},
            "global_blocked":["skill:test:blocked"],
            "actor_hidden":{(group_id.clone()):{"user":["skill:test:hidden"]}}
        }),
    );

    let blocked = call(
        &home,
        "capability_overview",
        json!({"group_id":group_id,"policy":"blocked"}),
    );
    assert_eq!(blocked["total_count"], 1);
    assert_eq!(blocked["items"][0]["capability_id"], "skill:test:blocked");
    assert_eq!(blocked["items"][0]["qualification_status"], "blocked");

    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,
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
            "group_id":group_id,
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
            "group_id":group_id,
            "actor_id":"user",
            "capability_id":"skill:test:hidden",
            "hidden":false
        }),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group_id,"actor_id":"user"}),
    );
    assert_eq!(state["enabled_capabilities"], json!([]));
    assert_eq!(state["actor_hidden_capabilities"], json!([]));
    let stored: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert!(stored["group_enabled"].get(&group_id).is_none());
    assert!(
        stored["global_blocked"]
            .as_object()
            .expect("global")
            .is_empty()
    );
    assert!(stored["actor_hidden"].get(&group_id).is_none());

    let blocked = call(
        &home,
        "capability_overview",
        json!({"group_id":group_id,"policy":"blocked"}),
    );
    assert_eq!(blocked["total_count"], 0);
    assert_eq!(blocked["blocked_capabilities"], json!([]));
}

#[test]
fn local_skill_uninstall_is_group_scoped_and_reenable_clears_marker() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability lifecycle", "")
        .expect("group");
    let other_group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability lifecycle other", "")
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
    call(
        &home,
        "capability_enable",
        json!({
            "group_id":other_group.group_id,"capability_id":"skill:local:review",
            "scope":"group","enabled":true,"by":"user"
        }),
    );

    let removed = call(
        &home,
        "capability_uninstall",
        json!({"group_id":group.group_id,"capability_id":"skill:local:review","by":"user"}),
    );
    assert_eq!(removed["removed_record"], false);
    assert!(removed["removed_bindings"].as_u64().unwrap_or(0) > 0);
    assert_eq!(removed["removed_group_marker"], true);
    assert_eq!(removed["removed_installation"], false);
    assert_eq!(
        removed["cleanup_skipped_reason"],
        "cleanup_skipped_capability_still_bound"
    );
    let record: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/catalog.json")).expect("catalog"),
    )
    .expect("catalog JSON");
    assert!(record["records"].get("skill:local:review").is_some());
    let state: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert_eq!(
        state["group_removed"][&group.group_id],
        json!(["skill:local:review"])
    );
    assert_eq!(
        state["group_enabled"][&other_group.group_id],
        json!(["skill:local:review"])
    );

    let removed_overview = call(
        &home,
        "capability_overview",
        json!({"group_id":group.group_id,"query":"skill:local:review"}),
    );
    assert_eq!(removed_overview["total_count"], 0);
    let other_overview = call(
        &home,
        "capability_overview",
        json!({"group_id":other_group.group_id,"query":"skill:local:review"}),
    );
    assert_eq!(other_overview["total_count"], 1);
    let removed_search = call(
        &home,
        "capability_search",
        json!({"group_id":group.group_id,"query":"skill:local:review"}),
    );
    assert_eq!(removed_search["capabilities"], json!([]));

    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"capability_id":"skill:local:review",
            "scope":"group","enabled":true,"by":"user"
        }),
    );
    let state: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert!(state["group_removed"].get(&group.group_id).is_none());

    let deleted = call(
        &home,
        "capability_source_delete",
        json!({
            "group_id":group.group_id,"source_id":"local_import","by":"user"
        }),
    );
    assert_eq!(deleted["removed_records"], 1);
    assert_eq!(
        deleted["removed_capability_ids"],
        json!(["skill:local:review"])
    );
    let catalog: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/catalog.json")).expect("catalog"),
    )
    .expect("catalog JSON");
    assert!(catalog["records"].get("skill:local:review").is_none());
    let state: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert!(!state.to_string().contains("skill:local:review"));
}

#[test]
fn target_install_is_the_only_daemon_operation_name() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("canonical capability install", "")
        .expect("group");
    let args = json!({"group_id":group.group_id,"by":"user"});

    let canonical = response(&home, "capability_install_target", args.clone());
    assert_eq!(
        canonical.error.expect("missing target").code,
        "missing_install_target"
    );

    let removed_alias = response(&home, "capability_install", args);
    assert_eq!(
        removed_alias.error.expect("removed alias").code,
        "unknown_op"
    );
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
            "capability_id":"skill:test:existing","kind":"skill","name":"Original",
            "capsule_text":"Original capsule"
        }))
        .expect("existing record");
    let overwrite = response(
        &home,
        "capability_import",
        json!({
            "group_id":group.group_id,"by":"user","dry_run":true,
            "record":{
                "capability_id":"skill:test:existing","kind":"skill","name":"Changed",
                "capsule_text":"Changed capsule"
            }
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

#[test]
fn capability_scope_mutations_enforce_actor_and_foreman_boundaries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"capability access"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"lead","by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer","by":"user"}),
    );

    let peer_group = response(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"peer",
            "capability_id":"pack:space","scope":"group","enabled":true
        }),
    );
    assert_eq!(
        peer_group.error.expect("peer group denial").code,
        "permission_denied"
    );

    let peer_other_actor = response(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"lead",
            "capability_id":"pack:diagnostics","scope":"actor","enabled":true
        }),
    );
    assert_eq!(
        peer_other_actor.error.expect("cross-actor denial").code,
        "permission_denied"
    );

    let peer_self = response(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"peer",
            "capability_id":"pack:diagnostics","scope":"actor","enabled":true
        }),
    );
    assert!(peer_self.ok, "{:?}", peer_self.error);

    let foreman_group = response(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,"by":"lead","actor_id":"lead",
            "capability_id":"pack:space","scope":"group","enabled":true
        }),
    );
    assert!(foreman_group.ok, "{:?}", foreman_group.error);

    let missing_target = temp.path().join("not-created");
    let peer_install = response(
        &home,
        "capability_install_target",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"peer",
            "target":missing_target,"scope":"group"
        }),
    );
    assert_eq!(
        peer_install.error.expect("peer install denial").code,
        "permission_denied",
        "authorization must run before target inspection"
    );

    let peer_global_block = response(
        &home,
        "capability_block",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"peer",
            "capability_id":"pack:space","scope":"global","blocked":true
        }),
    );
    assert_eq!(
        peer_global_block.error.expect("global block denial").code,
        "permission_denied"
    );

    let peer_uninstall = response(
        &home,
        "capability_uninstall",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"peer",
            "capability_id":"pack:space"
        }),
    );
    assert_eq!(
        peer_uninstall.error.expect("peer uninstall denial").code,
        "permission_denied"
    );

    let peer_source_delete = response(
        &home,
        "capability_source_delete",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"peer",
            "source_id":"manual_import"
        }),
    );
    assert_eq!(
        peer_source_delete
            .error
            .expect("peer source deletion denial")
            .code,
        "permission_denied"
    );

    let peer_other_visibility = response(
        &home,
        "capability_visibility",
        json!({
            "group_id":group_id,"by":"peer","actor_id":"lead",
            "capability_id":"pack:space","hidden":true
        }),
    );
    assert_eq!(
        peer_other_visibility
            .error
            .expect("cross-actor visibility denial")
            .code,
        "permission_denied"
    );
}

#[test]
fn capability_import_normalizes_qualification_and_preserves_valid_record_on_rejection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability import contract", "")
        .expect("group");
    let capability_id = "skill:agent_self_proposed:review-flow";

    let thin = response(
        &home,
        "capability_import",
        json!({
            "group_id":group.group_id,"by":"user","dry_run":true,"probe":false,
            "record":{
                "capability_id":capability_id,"kind":"skill",
                "source_id":"agent_self_proposed","name":"Review Flow",
                "capsule_text":"Use a review checklist."
            }
        }),
    );
    assert!(thin.ok, "{:?}", thin.error);
    assert_eq!(thin.result["record"]["qualification_status"], "blocked");
    assert_eq!(thin.result["enableable_now"], false);
    assert_eq!(
        thin.result["readiness_preview"]["preview_status"],
        "blocked"
    );
    assert!(
        thin.result["record"]["qualification_reasons"]
            .as_array()
            .expect("qualification reasons")
            .iter()
            .any(|value| value
                .as_str()
                .is_some_and(|value| value.starts_with("missing_agent_self_proposed_sections:")))
    );
    assert!(!home.root().join("state/capabilities/catalog.json").exists());

    let capsule = concat!(
        "When to use: repeated reviews.\n",
        "Avoid when: no evidence exists.\n",
        "Procedure: inspect, test, report.\n",
        "Pitfalls: do not assume.\n",
        "Verification: rerun the same reproduction.\n"
    );
    let valid_args = json!({
        "group_id":group.group_id,"by":"user","probe":false,
        "record":{
            "capability_id":capability_id,"kind":"skill",
            "source_id":"agent_self_proposed","name":"Review Flow",
            "capsule_text":capsule
        }
    });
    let created = response(&home, "capability_import", valid_args.clone());
    assert!(created.ok, "{:?}", created.error);
    assert_eq!(created.result["import_action"], "created");
    assert_eq!(created.result["record_changed"], false);
    assert_eq!(created.result["record"]["origin_group_id"], group.group_id);
    assert_eq!(
        created.result["record"]["qualification_status"],
        "qualified"
    );
    assert_eq!(created.result["probe"]["state"], "skipped");

    let unchanged = response(&home, "capability_import", valid_args);
    assert!(unchanged.ok, "{:?}", unchanged.error);
    assert_eq!(unchanged.result["import_action"], "unchanged");
    assert_eq!(unchanged.result["record_changed"], false);

    let rejected = response(
        &home,
        "capability_import",
        json!({
            "group_id":group.group_id,"by":"user","probe":false,
            "record":{
                "capability_id":capability_id,"kind":"skill",
                "source_id":"agent_self_proposed","name":"Broken replacement",
                "capsule_text":"Procedure only."
            }
        }),
    );
    let error = rejected.error.expect("missing-section rejection");
    assert_eq!(error.code, "capability_import_invalid");
    assert_eq!(error.details["active_record_preserved"], true);
    let stored = cccc_core::capabilities::CapabilityStore::new(home.clone())
        .catalog_record(capability_id)
        .expect("catalog")
        .expect("preserved record");
    assert_eq!(stored["name"], "Review Flow");
    assert_eq!(stored["capsule_text"], capsule.trim_end());
}

#[test]
fn capability_import_rejects_missing_skill_capsule_and_wrong_self_proposed_namespace() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability import validation", "")
        .expect("group");

    let missing_capsule = response(
        &home,
        "capability_import",
        json!({
            "group_id":group.group_id,"by":"user","dry_run":true,
            "record":{"capability_id":"skill:manual:missing","kind":"skill"}
        }),
    );
    assert_eq!(
        missing_capsule.error.expect("missing capsule").code,
        "capability_import_invalid"
    );

    let wrong_namespace = response(
        &home,
        "capability_import",
        json!({
            "group_id":group.group_id,"by":"user","dry_run":true,
            "record":{
                "capability_id":"skill:github:collision","kind":"skill",
                "source_id":"agent_self_proposed","capsule_text":"complete enough"
            }
        }),
    );
    assert_eq!(
        wrong_namespace.error.expect("namespace rejection").code,
        "capability_import_invalid"
    );
    assert!(!home.root().join("state/capabilities/catalog.json").exists());
}

#[test]
fn actor_start_applies_and_projects_role_profile_and_actor_autoload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("startup capability baseline", "")
        .expect("group");
    let group_id = group.group_id;
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    call(
        &home,
        "actor_profile_upsert",
        json!({
            "by":"user",
            "profile":{
                "id":"autoload-profile",
                "name":"Autoload Profile",
                "runtime":"web_model",
                "runner":"headless",
                "command":[],
                "submit":"enter",
                "capability_defaults":{
                    "autoload_capabilities":["pack:space"],
                    "default_scope":"actor"
                }
            }
        }),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"lead1",
            "runtime":"web_model",
            "runner":"headless",
            "profile_id":"autoload-profile",
            "capability_autoload":["pack:context-advanced"],
            "by":"user"
        }),
    );

    let before = call(
        &home,
        "capability_state",
        json!({"group_id":group_id,"actor_id":"lead1","by":"lead1"}),
    );
    assert_eq!(
        before["actor_autoload_capabilities"],
        json!(["pack:context-advanced"])
    );
    assert_eq!(
        before["profile_autoload_capabilities"],
        json!(["pack:space"])
    );
    assert_eq!(
        before["autoload_capabilities"],
        json!(["pack:space", "pack:context-advanced"])
    );
    assert_eq!(before["enabled_capabilities"], json!([]));

    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"lead1","by":"user"}),
    );
    let after = call(
        &home,
        "capability_state",
        json!({"group_id":group_id,"actor_id":"lead1","by":"lead1"}),
    );
    let enabled = after["enabled_capabilities"]
        .as_array()
        .expect("enabled capabilities");
    for capability_id in [
        "pack:group-runtime",
        "pack:diagnostics",
        "pack:space",
        "pack:context-advanced",
    ] {
        assert!(
            enabled.contains(&json!(capability_id)),
            "missing {capability_id}"
        );
    }
}

#[test]
fn failed_actor_start_keeps_the_durable_autoload_baseline() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("failed startup capability baseline", "")
        .expect("group");
    let group_id = group.group_id;
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"lead1",
            "runtime":"custom",
            "runner":"pty",
            "command":["cccc-audit-command-that-does-not-exist"],
            "capability_autoload":["pack:space"],
            "by":"user"
        }),
    );
    let scope = temp.path().join("missing-after-attach");
    std::fs::create_dir(&scope).expect("scope");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":scope,"by":"user"}),
    );
    std::fs::remove_dir(&scope).expect("remove scope");

    let started = response(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"lead1","by":"user"}),
    );
    assert!(!started.ok);
    assert_eq!(
        started.error.expect("start failure").code,
        "invalid_project_root"
    );
    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group_id,"actor_id":"lead1","by":"lead1"}),
    );
    let enabled = state["enabled_capabilities"]
        .as_array()
        .expect("enabled capabilities");
    assert!(enabled.contains(&json!("pack:space")));
    assert_eq!(state["actor_autoload_capabilities"], json!(["pack:space"]));
}

#[test]
fn actor_configured_hidden_skill_is_projected_without_being_disabled() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("actor capability visibility", "")
        .expect("group");
    let group_id = group.group_id;
    let capability_id = "skill:manual:visibility-audit";
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    call(
        &home,
        "capability_import",
        json!({
            "group_id":group_id,
            "by":"user",
            "probe":false,
            "record":{
                "capability_id":capability_id,
                "kind":"skill",
                "source_id":"manual_import",
                "name":"Visibility audit",
                "capsule_text":"When to use: audit menus.\nAvoid when: no actor exists.\nProcedure: inspect visibility.\nPitfalls: hiding is not disabling.\nVerification: compare projections."
            }
        }),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runtime":"custom",
            "runner":"pty",
            "command":["sh","-c","exit 0"],
            "capability_hidden":[capability_id],
            "by":"user"
        }),
    );
    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "by":"peer1",
            "capability_id":capability_id,
            "scope":"actor",
            "enabled":true
        }),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    );
    assert!(
        state["enabled_capabilities"]
            .as_array()
            .expect("enabled")
            .contains(&json!(capability_id))
    );
    assert!(
        state["actor_hidden_capabilities"]
            .as_array()
            .expect("hidden")
            .contains(&json!(capability_id))
    );
    assert!(
        !state["active_capsule_skills"]
            .as_array()
            .expect("active capsules")
            .iter()
            .any(|row| row["capability_id"] == capability_id)
    );
    assert!(
        state["hidden_capabilities"]
            .as_array()
            .expect("hidden reasons")
            .iter()
            .any(|row| {
                row["capability_id"] == capability_id && row["reason"] == "actor_hidden"
            })
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
