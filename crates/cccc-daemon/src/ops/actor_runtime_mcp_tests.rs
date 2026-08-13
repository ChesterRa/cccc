use cccc_contracts::{Actor, ActorRuntime, GroupState, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, Scope, actors};

use super::actor_runtime_tests::request;
use super::{local_headless, runtime_restore};

#[cfg(unix)]
#[test]
fn headless_actor_start_fails_closed_when_mcp_setup_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("headless MCP preflight", "").expect("group");
    let marker = temp.path().join("provider-started");
    configure_broken_headless_claude(&store, &group.group_id, temp.path(), &marker, false);

    let response = request(&home, "actor_start", &group.group_id);

    assert!(!response.ok);
    assert!(
        response
            .error
            .expect("MCP setup error")
            .code
            .starts_with("runtime_mcp_")
    );
    assert!(!marker.exists());
    assert!(!local_headless::running(&group.group_id, "peer1"));
}

#[cfg(unix)]
#[test]
fn restore_does_not_launch_headless_actor_when_mcp_setup_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store
        .create("restore headless MCP preflight", "")
        .expect("group");
    let marker = temp.path().join("restored-provider-started");
    configure_broken_headless_claude(&store, &group.group_id, temp.path(), &marker, true);

    runtime_restore::restore_running(&home).expect("restore");

    assert!(!marker.exists());
    assert!(!local_headless::running(&group.group_id, "peer1"));
}

#[cfg(unix)]
fn configure_broken_headless_claude(
    store: &GroupStore,
    group_id: &str,
    project: &std::path::Path,
    marker: &std::path::Path,
    persisted_running: bool,
) {
    use std::os::unix::fs::PermissionsExt;

    let bin = project.join("broken-claude-bin");
    std::fs::create_dir(&bin).expect("fake CLI directory");
    let claude = bin.join("claude");
    std::fs::write(
        &claude,
        r#"#!/bin/sh
if [ "$1" = mcp ]; then
  exit 23
fi
: > "$CCCC_TEST_PROVIDER_STARTED"
while IFS= read -r line; do :; done
"#,
    )
    .expect("fake Claude CLI");
    let mut permissions = claude.metadata().expect("fake CLI metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&claude, permissions).expect("fake CLI permissions");

    store
        .mutate(group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: project.to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new("peer1");
            actor.runtime = ActorRuntime::Claude;
            actor.runner = RunnerKind::Headless;
            actor.command = vec![claude.to_string_lossy().into_owned()];
            actor
                .env
                .insert("PATH".into(), bin.to_string_lossy().into_owned());
            actor.env.insert(
                "CCCC_TEST_PROVIDER_STARTED".into(),
                marker.to_string_lossy().into_owned(),
            );
            actors::add(group, actor)?;
            group.running = persisted_running;
            group.state = GroupState::Active;
            Ok(())
        })
        .expect("configure broken headless Claude");
}
