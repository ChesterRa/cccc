use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use cccc_core::{Registry, active};
use serde_json::{Value, json};

fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().expect("args"),
    }
}

#[test]
fn create_response_keeps_group_and_exposes_top_level_group_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let response =
        cccc_daemon::handle_request(&home, &request("group_create", json!({"title":"demo"})));
    assert!(response.ok, "{:?}", response.error);
    assert_eq!(
        response.result["group_id"],
        response.result["group"]["group_id"]
    );
}

#[test]
fn create_with_scope_is_visible_only_after_attach_succeeds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let target = temp.path().join("project");
    let response = cccc_daemon::handle_request(
        &home,
        &request(
            "group_create_with_scope",
            json!({"title":"demo","path":target}),
        ),
    );
    assert!(response.ok, "{:?}", response.error);
    let group_id = response.result["group_id"].as_str().expect("group id");
    let group = GroupStore::new(home)
        .expect("store")
        .load(group_id)
        .expect("group");
    assert_eq!(group.scopes.len(), 1);
    assert_eq!(group.active_scope_key, group.scopes[0].scope_key);
    assert_eq!(
        group.scopes[0].url,
        target.canonicalize().expect("target").to_string_lossy()
    );
}

#[test]
fn same_scope_can_create_distinct_groups_and_latest_is_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let target = temp.path().join("project");
    std::fs::create_dir(&target).expect("project");
    let first = cccc_daemon::handle_request(
        &home,
        &request(
            "group_create_with_scope",
            json!({"title":"first","path":target}),
        ),
    );
    assert!(first.ok, "{:?}", first.error);
    let first_id = first.result["group_id"]
        .as_str()
        .expect("first id")
        .to_owned();
    let store = GroupStore::new(home.clone()).expect("store");

    let second = cccc_daemon::handle_request(
        &home,
        &request(
            "group_create_with_scope",
            json!({"title":"second","path":target}),
        ),
    );

    assert!(second.ok, "{:?}", second.error);
    let second_id = second.result["group_id"]
        .as_str()
        .expect("second id")
        .to_owned();
    assert_ne!(second_id, first_id);
    let first_group = store.load(&first_id).expect("first group");
    let second_group = store.load(&second_id).expect("second group");
    assert_eq!(first_group.title, "first");
    assert_eq!(second_group.title, "second");
    assert_eq!(first_group.scopes.len(), 1);
    assert_eq!(second_group.scopes.len(), 1);
    assert_eq!(first_group.scopes[0], second_group.scopes[0]);
    assert_eq!(store.list().expect("groups").len(), 2);
    assert_eq!(
        Registry::load(&home)
            .expect("registry")
            .defaults
            .get(&first_group.scopes[0].scope_key),
        Some(&second_id)
    );
    assert_eq!(active::get(&home).expect("active"), Some(second_id));
    assert_eq!(std::fs::read_dir(&target).expect("target").count(), 0);
}

#[test]
fn relative_create_path_is_rejected_before_daemon_cwd_can_affect_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let web_cwd = temp.path().join("web-cwd");
    let daemon_cwd = temp.path().join("daemon-cwd");
    std::fs::create_dir(&web_cwd).expect("web cwd");
    std::fs::create_dir(&daemon_cwd).expect("daemon cwd");
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .current_dir(&daemon_cwd)
        .env("CCCC_RELATIVE_PATH_CHILD_HOME", temp.path().join("home"))
        .arg("relative_path_child_rejects_without_using_its_cwd")
        .arg("--exact")
        .status()
        .expect("child test");
    assert!(status.success());
    assert!(!web_cwd.join("missing-parent").exists());
    assert!(!daemon_cwd.join("missing-parent").exists());
}

#[test]
fn relative_path_child_rejects_without_using_its_cwd() {
    let Some(home) = std::env::var_os("CCCC_RELATIVE_PATH_CHILD_HOME") else {
        return;
    };
    let home = HomeLayout::from_path(home).expect("home");
    let response = cccc_daemon::handle_request(
        &home,
        &request(
            "group_create_with_scope",
            json!({"title":"relative","path":"missing-parent/project"}),
        ),
    );
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "invalid_path");
    assert!(
        GroupStore::new(home)
            .expect("store")
            .list()
            .expect("groups")
            .is_empty()
    );
}
