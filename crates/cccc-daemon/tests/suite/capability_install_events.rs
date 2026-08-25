use cccc_core::{GroupStore, HomeLayout};
use serde_json::json;

use super::capability_install_events_support::{call, capability_events, response, write_skill};

#[test]
fn successful_install_publishes_one_durable_change_after_state_is_visible() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("install events", "")
        .expect("group");
    let skill_dir = write_skill(temp.path(), "review", "Review changes", "Review carefully.");

    let installed = call(
        &home,
        "capability_install_target",
        json!({
            "group_id":group.group_id,"target":skill_dir,
            "scope":"group","by":"user"
        }),
    );

    assert_eq!(installed["refresh_required"], true);
    let events = capability_events(&home, &group.group_id);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data["capability_id"], "skill:local:review");
    assert_eq!(
        events[0].data["capability_ids"],
        json!(["skill:local:review"])
    );
    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        state["active_capsule_skills"]
            .as_array()
            .expect("active skills")
            .iter()
            .any(|row| row["capability_id"] == "skill:local:review")
    );

    let repeated = call(
        &home,
        "capability_install_target",
        json!({
            "group_id":group.group_id,"target":skill_dir,
            "scope":"group","by":"user"
        }),
    );
    assert_eq!(repeated["refresh_required"], false);
    assert_eq!(capability_events(&home, &group.group_id).len(), 1);
}

#[test]
fn reinstall_unhides_atomically_and_publishes_one_new_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("install visibility", "")
        .expect("group");
    let skill_dir = write_skill(temp.path(), "review", "Review changes", "Review carefully.");
    let args = json!({
        "group_id":group.group_id,"target":skill_dir,
        "scope":"group","by":"user"
    });
    call(&home, "capability_install_target", args.clone());
    call(
        &home,
        "capability_visibility",
        json!({
            "group_id":group.group_id,"actor_id":"user","by":"user",
            "capability_id":"skill:local:review","hidden":true
        }),
    );

    let installed = call(&home, "capability_install_target", args);

    assert_eq!(installed["refresh_required"], true);
    assert_eq!(capability_events(&home, &group.group_id).len(), 2);
    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert_eq!(state["actor_hidden_capabilities"], json!([]));
    assert!(
        state["active_capsule_skills"]
            .as_array()
            .expect("active skills")
            .iter()
            .any(|row| row["capability_id"] == "skill:local:review")
    );
}

#[test]
fn blocked_reinstall_publishes_a_catalog_change_without_claiming_ready() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("blocked update", "")
        .expect("group");
    let skill_dir = write_skill(temp.path(), "review", "Review changes", "First version.");
    let args = json!({
        "group_id":group.group_id,"target":skill_dir,
        "scope":"group","by":"user"
    });
    call(&home, "capability_install_target", args.clone());
    cccc_core::capabilities::CapabilityStore::new(home.clone())
        .set_blocked_for(
            "skill:local:review",
            true,
            &group.group_id,
            "test policy",
            "user",
            0,
        )
        .expect("block");
    write_skill(temp.path(), "review", "Review changes", "Second version.");

    let installed = call(&home, "capability_install_target", args);

    assert_eq!(installed["state"], "needs_setup");
    assert_eq!(installed["refresh_required"], true);
    let events = capability_events(&home, &group.group_id);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].data["state"], "needs_setup");
}

#[test]
fn failed_install_does_not_publish_a_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("failed install", "")
        .expect("group");
    let skill_dir = write_skill(temp.path(), "blocked", "", "Missing description.");

    let installed = response(
        &home,
        "capability_install_target",
        json!({
            "group_id":group.group_id,"target":skill_dir,
            "scope":"group","by":"user"
        }),
    );

    assert!(!installed.ok);
    assert!(capability_events(&home, &group.group_id).is_empty());
}

#[test]
fn state_write_failure_rolls_back_the_record_without_publishing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("rollback install", "")
        .expect("group");
    let state_path = home.root().join("state/capabilities/state.json");
    std::fs::create_dir_all(&state_path).expect("block state write");
    let skill_dir = write_skill(temp.path(), "review", "Review changes", "Review carefully.");

    let installed = response(
        &home,
        "capability_install_target",
        json!({
            "group_id":group.group_id,"target":skill_dir,
            "scope":"group","by":"user"
        }),
    );

    assert!(!installed.ok);
    assert!(capability_events(&home, &group.group_id).is_empty());
    assert!(
        cccc_core::capabilities::CapabilityStore::new(home)
            .catalog_record("skill:local:review")
            .expect("catalog")
            .is_none()
    );
}

#[test]
fn event_write_failure_keeps_the_committed_install_successful() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("groups");
    let group = store.create("event failure", "").expect("group");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    if ledger_path.exists() {
        std::fs::remove_file(&ledger_path).expect("remove ledger file");
    }
    std::fs::create_dir_all(&ledger_path).expect("block ledger append");
    let skill_dir = write_skill(temp.path(), "review", "Review changes", "Review carefully.");

    let installed = response(
        &home,
        "capability_install_target",
        json!({
            "group_id":group.group_id,"target":skill_dir,
            "scope":"group","by":"user"
        }),
    );

    assert!(installed.ok, "{:?}", installed.error);
    assert!(installed.result["event_publish_error"].as_str().is_some());
    let state = call(
        &home,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        state["active_capsule_skills"]
            .as_array()
            .expect("active skills")
            .iter()
            .any(|row| row["capability_id"] == "skill:local:review")
    );
}
