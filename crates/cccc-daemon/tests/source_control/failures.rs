use super::*;

#[test]
fn partial_pause_failure_survives_hooks_and_restart_until_explicit_repair() {
    let temp = tempfile::tempdir().expect("Home");
    let home = HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("init");
    let a = source(&home, "partial");
    let b = source(&home, "other");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("kept", "").expect("group");
    let mut actor = Actor::new("member");
    actor.runtime = ActorRuntime::Codex;
    actor.external_session_id = a.source_id.clone();
    actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    let path = store
        .group_dir(&group.group_id)
        .expect("dir")
        .join("group.yaml");
    let original = std::fs::read(&path).expect("yaml");
    std::fs::write(&path, b"actors: [broken").expect("actual unreadable Group");
    let response = call(
        &home,
        "local_session_control",
        json!({"source_id":a.source_id,"action":"remove","expected_revision":"0","by":"user"}),
    );
    assert!(response.ok, "{:?}", response.error);
    assert_eq!(response.result["status"], "partial");
    assert_eq!(response.result["access_blocked"], true);
    assert_eq!(response.result["pause_complete"], false);
    assert_eq!(response.result["failed_group_ids"], json!([group.group_id]));
    let reopened = HomeLayout::from_path(home.root()).expect("reopen");
    assert!(local_sessions::read(&reopened, &a.source_id).is_err());
    assert!(local_sessions::write(&reopened, a.clone()).is_err());
    assert_eq!(
        local_sessions::read(&home, &b.source_id).expect("other unaffected"),
        b
    );
    let revision = response.result["management_revision"].clone();
    let failed_restore = call(
        &reopened,
        "local_session_control",
        json!({"source_id":a.source_id,"action":"restore","expected_revision":revision,"by":"user"}),
    );
    assert_eq!(failed_restore.result["status"], "partial");
    assert_eq!(failed_restore.result["access_blocked"], true);
    std::fs::write(&path, original).expect("repair fixture");
    let restored = call(
        &reopened,
        "local_session_control",
        json!({"source_id":a.source_id,"action":"restore","expected_revision":revision,"by":"user"}),
    );
    assert!(restored.ok, "{:?}", restored.error);
    assert_eq!(restored.result["status"], "active");
    assert!(
        !store
            .load(&group.group_id)
            .expect("history retained")
            .actors[0]
            .enabled
    );
    assert!(a.host.alive());
}

#[test]
fn revisions_are_management_only_and_operations_are_idempotent() {
    let temp = tempfile::tempdir().expect("Home");
    let home = HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("init");
    let a = source(&home, "revision");
    let request = |action: &str, revision: Value| json!({"source_id":a.source_id,"action":action,"expected_revision":revision,"by":"user"});
    let removed = call(
        &home,
        "local_session_control",
        request("remove", json!("0")),
    );
    assert!(removed.ok, "{:?}", removed.error);
    let duplicate = call(
        &home,
        "local_session_control",
        request("remove", json!("0")),
    );
    assert_eq!(duplicate.result, removed.result);
    let conflict = call(
        &home,
        "local_session_control",
        request("restore", json!("0")),
    );
    assert_eq!(
        conflict.error.expect("stale intent").code,
        "source_control_conflict"
    );
    let restored = call(
        &home,
        "local_session_control",
        request("restore", removed.result["management_revision"].clone()),
    );
    assert!(restored.ok, "{:?}", restored.error);
    let again = call(
        &home,
        "local_session_control",
        request("restore", removed.result["management_revision"].clone()),
    );
    assert_eq!(again.result, restored.result);
    let list = call(&home, "local_session_list", json!({"include_offline":true}));
    assert!(list.ok);
    assert_eq!(list.result["capabilities"]["source_control"], true);
    assert_eq!(
        list.result["source_states"][&a.source_id]["control"]["management_revision"],
        restored.result["management_revision"]
    );
}

#[test]
fn management_rejects_native_cloud_remote_unknown_and_path_inputs() {
    let temp = tempfile::tempdir().expect("Home");
    let home = HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("init");
    let a = source(&home, "authority");
    for key in [
        "_cccc_native_binding",
        "_cccc_native_source_binding",
        "_cccc_web_binding",
    ] {
        let response = call(
            &home,
            "local_session_control",
            json!({"source_id":a.source_id,"action":"remove","expected_revision":"0","by":"user",key:{}}),
        );
        assert!(!response.ok);
        assert!(local_sessions::read(&home, &a.source_id).is_ok());
    }
    for (id, code) in [
        ("remote:device:grant:source", "source_control_unsupported"),
        ("codex:unknown", "not_found"),
        ("../../project", "invalid_args"),
    ] {
        let response = call(
            &home,
            "local_session_control",
            json!({"source_id":id,"action":"remove","expected_revision":"0","by":"user"}),
        );
        assert!(!response.ok);
        assert_eq!(response.error.expect("denial").code, code);
    }
    assert!(local_sessions::read(&home, &a.source_id).is_ok());
}

#[test]
fn repeated_remove_rechecks_members_even_after_an_explicit_enable_while_blocked() {
    let temp = tempfile::tempdir().expect("Home");
    let home = HomeLayout::from_path(temp.path()).expect("Home");
    home.initialize().expect("init");
    let a = source(&home, "reassert");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("kept", "").expect("group");
    let mut actor = Actor::new("member");
    actor.runtime = ActorRuntime::Codex;
    actor.external_session_id = a.source_id.clone();
    actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    let removed = call(
        &home,
        "local_session_control",
        json!({"source_id":a.source_id,"action":"remove","expected_revision":"0","by":"user"}),
    );
    assert!(removed.ok);
    let enabled = call(
        &home,
        "actor_start",
        json!({"group_id":group.group_id,"actor_id":group.actors[0].id,"by":"user"}),
    );
    assert!(enabled.ok, "{:?}", enabled.error);
    assert!(store.load(&group.group_id).expect("enabled flag").actors[0].enabled);
    assert!(
        local_sessions::read(&home, &a.source_id).is_err(),
        "source barrier remains authoritative"
    );
    let again = call(
        &home,
        "local_session_control",
        json!({"source_id":a.source_id,"action":"remove","expected_revision":"0","by":"user"}),
    );
    assert!(again.ok);
    assert_eq!(
        again.result["management_revision"],
        removed.result["management_revision"]
    );
    assert!(
        !store
            .load(&group.group_id)
            .expect("reconciled pause")
            .actors[0]
            .enabled
    );
}
