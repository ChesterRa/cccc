// Included by the crate-level integration test harness.
use cccc_contracts::DaemonRequest;
use cccc_core::GroupStore;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

const SELF_EVOLUTION_CAPABILITY_ID: &str = "skill:cccc:self-evolution";
const LEGACY_SELF_EVOLUTION_CAPABILITY_ID: &str = "skill:agent_self_proposed:cccc-self-evolution";

#[test]
fn self_evolution_is_builtin_and_default_enabled_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("self evolution", "")
        .expect("group");

    let first = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        first["enabled_capabilities"]
            .as_array()
            .expect("enabled")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );
    assert!(
        first["active_capsule_skills"]
            .as_array()
            .expect("skills")
            .iter()
            .any(|row| row["capability_id"] == SELF_EVOLUTION_CAPABILITY_ID)
    );
    let builtin = cccc_core::capabilities::CapabilityStore::new(home.clone())
        .require(SELF_EVOLUTION_CAPABILITY_ID)
        .expect("built-in self evolution");
    assert_eq!(builtin.source, "cccc_builtin");

    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"actor_id":"user","by":"user",
            "capability_id":SELF_EVOLUTION_CAPABILITY_ID,"scope":"group","enabled":false
        }),
    );
    for _ in 0..2 {
        let state = call(
            &home,
            "capability_state",
            json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
        );
        assert!(
            !state["enabled_capabilities"]
                .as_array()
                .expect("enabled")
                .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
        );
    }
}

#[test]
fn self_evolution_disable_before_first_state_read_is_durable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("early self evolution disable", "")
        .expect("group");

    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"actor_id":"user","by":"user",
            "capability_id":SELF_EVOLUTION_CAPABILITY_ID,"scope":"group","enabled":false
        }),
    );
    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        !state["enabled_capabilities"]
            .as_array()
            .expect("enabled")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );
}

#[test]
fn legacy_self_evolution_binding_migrates_without_duplicate_activation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("self evolution migration", "")
        .expect("group");
    std::fs::create_dir_all(home.root().join("state/capabilities")).expect("state dir");
    write_json(
        &home.root().join("state/capabilities/state.json"),
        json!({"v":1,"group_enabled":{(group.group_id.clone()):[LEGACY_SELF_EVOLUTION_CAPABILITY_ID]}}),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    let enabled = state["enabled_capabilities"].as_array().expect("enabled");
    assert!(enabled.contains(&json!(SELF_EVOLUTION_CAPABILITY_ID)));
    assert!(!enabled.contains(&json!(LEGACY_SELF_EVOLUTION_CAPABILITY_ID)));

    let persisted: Value =
        cccc_core::fs::read_json(&home.root().join("state/capabilities/state.json"))
            .expect("persisted state");
    assert!(
        persisted["group_removed"][&group.group_id]
            .as_array()
            .expect("removed")
            .contains(&json!(LEGACY_SELF_EVOLUTION_CAPABILITY_ID))
    );
}

#[test]
fn legacy_self_evolution_disable_migrates_to_the_builtin_capability() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("self evolution disable migration", "")
        .expect("group");
    std::fs::create_dir_all(home.root().join("state/capabilities")).expect("state dir");
    write_json(
        &home.root().join("state/capabilities/state.json"),
        json!({"v":1,"group_removed":{(group.group_id.clone()):[LEGACY_SELF_EVOLUTION_CAPABILITY_ID]}}),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        !state["enabled_capabilities"]
            .as_array()
            .expect("enabled")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );

    let persisted: Value =
        cccc_core::fs::read_json(&home.root().join("state/capabilities/state.json"))
            .expect("persisted state");
    assert!(
        persisted["group_removed"][&group.group_id]
            .as_array()
            .expect("removed")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );
}

#[test]
fn legacy_self_evolution_controls_migrate_with_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("self evolution control migration", "")
        .expect("group");
    let global_block = json!({
        "reason":"global policy","by":"user",
        "blocked_at":"2026-08-25T00:00:00Z","expires_at":""
    });
    let group_block = json!({
        "reason":"group policy","by":"foreman",
        "blocked_at":"2026-08-25T01:00:00Z","expires_at":""
    });
    std::fs::create_dir_all(home.root().join("state/capabilities")).expect("state dir");
    write_json(
        &home.root().join("state/capabilities/state.json"),
        json!({
            "v":1,
            "default_group_capability_seed_versions":{(group.group_id.clone()):1},
            "global_blocked":{LEGACY_SELF_EVOLUTION_CAPABILITY_ID:global_block.clone()},
            "group_blocked":{
                (group.group_id.clone()):{LEGACY_SELF_EVOLUTION_CAPABILITY_ID:group_block.clone()}
            },
            "actor_hidden":{
                (group.group_id.clone()):{"user":[LEGACY_SELF_EVOLUTION_CAPABILITY_ID]}
            }
        }),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        !state["enabled_capabilities"]
            .as_array()
            .expect("enabled")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );
    assert!(
        state["actor_hidden_capabilities"]
            .as_array()
            .expect("hidden")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );

    let persisted: Value =
        cccc_core::fs::read_json(&home.root().join("state/capabilities/state.json"))
            .expect("persisted state");
    assert_eq!(
        persisted["global_blocked"][SELF_EVOLUTION_CAPABILITY_ID],
        global_block
    );
    assert_eq!(
        persisted["group_blocked"][&group.group_id][SELF_EVOLUTION_CAPABILITY_ID],
        group_block
    );
    assert!(
        persisted["actor_hidden"][&group.group_id]["user"]
            .as_array()
            .expect("hidden")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );
}

#[test]
fn blocked_default_self_evolution_is_not_active() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("blocked self evolution", "")
        .expect("group");
    call(
        &home,
        "capability_block",
        json!({
            "group_id":group.group_id,"actor_id":"user","by":"user",
            "capability_id":SELF_EVOLUTION_CAPABILITY_ID,"scope":"group",
            "blocked":true,"reason":"test"
        }),
    );

    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        !state["enabled_capabilities"]
            .as_array()
            .expect("enabled")
            .contains(&json!(SELF_EVOLUTION_CAPABILITY_ID))
    );
    assert!(
        !state["active_capsule_skills"]
            .as_array()
            .expect("skills")
            .iter()
            .any(|row| row["capability_id"] == SELF_EVOLUTION_CAPABILITY_ID)
    );
}

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
    assert_eq!(overview["total_count"], 2);
    assert_eq!(overview["kind_counts"]["skill"], 2);
    let review = overview["items"]
        .as_array()
        .expect("overview items")
        .iter()
        .find(|row| row["capability_id"] == "skill:test:review")
        .expect("legacy review skill");
    assert_eq!(review["kind"], "skill");
    assert_eq!(review["source_id"], "github_skills_curated");
    assert_eq!(review["source_uri"], "https://example.test/review");
    assert_eq!(review["autoload_candidate"], true);
}

#[test]
fn unsupported_catalog_records_are_not_autoload_candidates() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    std::fs::create_dir_all(home.root().join("state/capabilities")).expect("capability state");
    write_json(
        &home.root().join("state/capabilities/catalog.json"),
        json!({"records":{"skill:test:unsupported":{
            "capability_id":"skill:test:unsupported",
            "kind":"skill",
            "name":"unsupported",
            "qualification_status":"blocked",
            "enable_supported":false
        }}}),
    );

    let overview = call(
        &home,
        "capability_overview",
        json!({"query":"skill:test:unsupported"}),
    );

    assert_eq!(overview["total_count"], 1);
    assert_eq!(overview["items"][0]["qualification_status"], "blocked");
    assert_eq!(overview["items"][0]["enable_supported"], false);
    assert_eq!(overview["items"][0]["autoload_candidate"], false);
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
    assert_eq!(
        state["enabled_capabilities"],
        json!([SELF_EVOLUTION_CAPABILITY_ID])
    );
    assert_eq!(state["actor_hidden_capabilities"], json!([]));
    let stored: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert_eq!(
        stored["group_enabled"][&group_id],
        json!([SELF_EVOLUTION_CAPABILITY_ID])
    );
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
fn group_block_revokes_group_bindings_and_runtime_exposure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability block revocation", "")
        .expect("group");
    let capability_id = "pack:space";

    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"capability_id":capability_id,
            "scope":"group","enabled":true,"by":"user"
        }),
    );
    let runtime_path = home.root().join("state/capabilities/runtime.json");
    std::fs::create_dir_all(runtime_path.parent().expect("runtime parent")).expect("runtime dir");
    write_json(
        &runtime_path,
        json!({
            "v":2,
            "actor_instances":{
                (group.group_id.clone()):{
                    "peer-1":{
                        (capability_id):{
                            "artifact_id":"art_test","state":"ready"
                        }
                    }
                }
            }
        }),
    );

    let blocked = call(
        &home,
        "capability_block",
        json!({
            "group_id":group.group_id,"capability_id":capability_id,
            "scope":"group","blocked":true,"reason":"unsafe","by":"user"
        }),
    );

    assert!(
        blocked["action_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("cblk_"))
    );
    assert_eq!(blocked["state"], "blocked");
    assert_eq!(blocked["blocked"], true);
    assert_eq!(blocked["removed_bindings"], 1);
    assert_eq!(blocked["removed_runtime_bindings"], 1);
    assert_eq!(blocked["refresh_required"], true);
    let state: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert!(state["group_enabled"].get(&group.group_id).is_none());
    let runtime: Value = serde_json::from_slice(&std::fs::read(runtime_path).expect("runtime"))
        .expect("runtime JSON");
    assert!(
        runtime["actor_instances"]
            .get(&group.group_id)
            .and_then(|actors| actors.get("peer-1"))
            .and_then(|capabilities| capabilities.get(capability_id))
            .is_none()
    );

    let reenable = call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"capability_id":capability_id,
            "scope":"group","enabled":true,"by":"user"
        }),
    );
    assert_eq!(reenable["state"], "blocked");
    assert_eq!(reenable["enabled"], false);
    assert_eq!(reenable["reason"], "blocked_by_group_policy");
    let state: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/state.json")).expect("state"),
    )
    .expect("state JSON");
    assert!(state["group_enabled"].get(&group.group_id).is_none());
}

#[test]
fn expired_block_does_not_prevent_enable_or_effective_exposure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("expired capability block", "")
        .expect("group");
    let state_path = home.root().join("state/capabilities/state.json");
    std::fs::create_dir_all(state_path.parent().expect("state parent")).expect("state dir");
    write_json(
        &state_path,
        json!({
            "v":1,
            "group_blocked":{
                (group.group_id.clone()):{
                    "pack:space":{
                        "reason":"temporary","by":"user",
                        "blocked_at":"2000-01-01T00:00:00Z",
                        "expires_at":"2000-01-01T00:00:01Z"
                    }
                }
            }
        }),
    );

    let enabled = call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"capability_id":"pack:space",
            "scope":"group","enabled":true,"by":"user"
        }),
    );
    assert_ne!(enabled["state"], "blocked");
    assert_eq!(enabled["enabled"], true);
    let effective = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert_eq!(
        effective["enabled_capabilities"],
        json!(["pack:space", SELF_EVOLUTION_CAPABILITY_ID])
    );
}

#[test]
fn non_object_capability_state_fails_closed_without_overwrite() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("invalid capability state", "")
        .expect("group");
    let state_path = home.root().join("state/capabilities/state.json");
    std::fs::create_dir_all(state_path.parent().expect("state parent")).expect("state dir");
    write_json(&state_path, json!([]));
    let before = std::fs::read(&state_path).expect("state bytes");

    let result = response(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"capability_id":"pack:space",
            "scope":"group","enabled":true,"by":"user"
        }),
    );

    assert!(!result.ok);
    assert_eq!(std::fs::read(&state_path).expect("state bytes"), before);
}

#[test]
fn allowlist_expected_revision_is_checked_inside_the_write_lock() {
    use fs2::FileExt;
    use std::fs::OpenOptions;
    use std::sync::{Arc, Barrier};

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let revision = call(&home, "capability_allowlist_get", json!({}))["revision"]
        .as_str()
        .expect("revision")
        .to_owned();
    let lock_path = home
        .root()
        .join("config/capability-allowlist.user.yaml.lock");
    std::fs::create_dir_all(lock_path.parent().expect("lock parent")).expect("lock dir");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("lockfile");
    lock.lock_exclusive().expect("hold allowlist lock");

    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for (name, level) in [("source-a", "mounted"), ("source-b", "indexed")] {
        let home = home.clone();
        let revision = revision.clone();
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            response(
                &home,
                "capability_allowlist_update",
                json!({
                    "by":"user","mode":"patch","expected_revision":revision,
                    "patch":{"defaults":{"source_level":{(name):level}}}
                }),
            )
        }));
    }
    barrier.wait();
    std::thread::sleep(std::time::Duration::from_millis(150));
    FileExt::unlock(&lock).expect("release allowlist lock");

    let responses = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();
    assert_eq!(responses.iter().filter(|response| response.ok).count(), 1);
    assert_eq!(
        responses
            .iter()
            .filter_map(|response| response.error.as_ref())
            .filter(|error| error.code == "allowlist_revision_mismatch")
            .count(),
        1
    );
}

#[test]
fn capability_state_reports_assignment_usage_and_enforces_self_inspection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"capability usage"}));
    let group_id = created["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    for actor_id in ["peer-a", "peer-b"] {
        call(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        );
    }
    call(
        &home,
        "capability_enable",
        json!({
            "group_id":group_id,"by":"user","actor_id":"peer-a",
            "capability_id":"pack:space","scope":"actor","enabled":true
        }),
    );

    let state = call(
        &home,
        "capability_state",
        json!({
            "group_id":group_id,"by":"user","actor_id":"user",
            "capability_id":"pack:space"
        }),
    );
    let usage = &state["capability_usage"];
    assert_eq!(usage["capability_id"], "pack:space");
    assert_eq!(usage["used"], true);
    assert_eq!(usage["group_enabled"], false);
    assert_eq!(usage["group_actor_count"], 2);
    assert_eq!(usage["active_actor_count"], 1);
    assert_eq!(usage["actor_enabled"][0]["actor_id"], "peer-a");

    let denied = response(
        &home,
        "capability_state",
        json!({"group_id":group_id,"by":"peer-b","actor_id":"peer-a"}),
    );
    assert_eq!(
        denied.error.expect("cross-actor inspection denial").code,
        "permission_denied"
    );
}

#[test]
fn capability_enable_returns_the_declared_lifecycle_shape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("capability enable receipt", "")
        .expect("group");

    let enabled = call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"capability_id":"pack:space",
            "scope":"group","enabled":true,"by":"user"
        }),
    );
    assert!(
        enabled["action_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("cact_"))
    );
    assert_eq!(enabled["group_id"], group.group_id);
    assert_eq!(enabled["actor_id"], "user");
    assert_eq!(enabled["scope"], "group");
    assert_eq!(enabled["enabled"], true);
    assert_eq!(enabled["state"], "activation_pending");
    assert_eq!(enabled["refresh_required"], true);

    let disabled = call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"capability_id":"pack:space",
            "scope":"group","enabled":false,"by":"user"
        }),
    );
    assert!(
        disabled["action_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("cact_"))
    );
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["state"], "disabled");
    assert_eq!(disabled["refresh_required"], true);
}

#[test]
fn external_stdio_capability_completes_the_mcp_initialized_handshake() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("strict MCP lifecycle", "")
        .expect("group");
    let server = temp.path().join("strict_mcp.py");
    std::fs::write(
        &server,
        r#"import json, sys
initialized = False
for raw in sys.stdin:
    message = json.loads(raw)
    method = message.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":message["id"],"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"strict","version":"1"}}}), flush=True)
    elif method == "notifications/initialized":
        initialized = True
    elif method == "tools/list":
        if not initialized:
            print(json.dumps({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32002,"message":"not initialized"}}), flush=True)
        else:
            print(json.dumps({"jsonrpc":"2.0","id":message["id"],"result":{"tools":[{"name":"echo","description":"Echo","inputSchema":{"type":"object","properties":{}}}]}}), flush=True)
    elif method == "tools/call":
        if not initialized:
            print(json.dumps({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32002,"message":"not initialized"}}), flush=True)
        else:
            print(json.dumps({"jsonrpc":"2.0","id":message["id"],"result":{"content":[{"type":"text","text":"ok"}]}}), flush=True)
"#,
    )
    .expect("strict MCP fixture");
    cccc_core::capabilities::CapabilityStore::new(home.clone())
        .import_record(json!({
            "capability_id":"mcp:test:strict","kind":"mcp_toolpack","name":"Strict MCP",
            "source_id":"manual_import","qualification_status":"qualified",
            "enable_supported":true,"install_mode":"command",
            "install_spec":{"command_candidates":[
                ["python3",server.to_string_lossy()],
                ["python",server.to_string_lossy()]
            ]}
        }))
        .expect("catalog record");

    let enabled = call(
        &home,
        "capability_enable",
        json!({
            "group_id":group.group_id,"actor_id":"user","by":"user",
            "capability_id":"mcp:test:strict","scope":"actor","enabled":true
        }),
    );
    assert_eq!(enabled["state"], "activation_pending");
    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    let tool_name = state["dynamic_tools"][0]["name"]
        .as_str()
        .expect("dynamic tool")
        .to_owned();
    let invoked = call(
        &home,
        "capability_tool_call",
        json!({
            "group_id":group.group_id,"actor_id":"user","by":"user",
            "tool_name":tool_name,"arguments":{"value":"hello"}
        }),
    );
    assert_eq!(invoked["capability_id"], "mcp:test:strict");
    assert_eq!(invoked["result"]["content"][0]["text"], "ok");
    let runtime: Value = serde_json::from_slice(
        &std::fs::read(home.root().join("state/capabilities/runtime.json")).expect("runtime"),
    )
    .expect("runtime JSON");
    assert_eq!(
        runtime["actor_instances"][&group.group_id]["user"]["mcp:test:strict"]["state"],
        "verified"
    );
    assert_eq!(
        runtime["recent_success"]["mcp:test:strict"]["success_count"],
        1
    );
    let after = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert_eq!(after["dynamic_tools"][0]["name"], tool_name);
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
    assert_eq!(
        before["enabled_capabilities"],
        json!([SELF_EVOLUTION_CAPABILITY_ID])
    );

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
