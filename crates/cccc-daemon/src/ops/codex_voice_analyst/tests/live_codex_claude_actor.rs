use super::live_support::{
    assert_managed_actor_starts_idle, run_busy_actor_delivery_canary, wait_for_daemon,
};
use cccc_contracts::{Actor, ActorRole, ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, Scope, actors};
use serde_json::Value;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_codex_group_actor_accepts_delivery_while_busy_when_explicitly_enabled() {
    if std::env::var("CCCC_CODEX_MANAGED_ACTOR_LIVE").as_deref() != Ok("1") {
        return;
    }
    run_canary(
        ActorRuntime::Codex,
        "CCCC_CODEX_EXECUTABLE",
        "codex",
        "codex_app_server",
        "CODEX_BUSY_DELIVERY_REPLY",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_claude_group_actor_accepts_delivery_while_busy_when_explicitly_enabled() {
    if std::env::var("CCCC_CLAUDE_MANAGED_LIVE").as_deref() != Ok("1") {
        return;
    }
    run_canary(
        ActorRuntime::Claude,
        "CCCC_CLAUDE_EXECUTABLE",
        "claude",
        "claude_agent_view",
        "CLAUDE_BUSY_DELIVERY_REPLY",
    )
    .await;
}

async fn run_canary(
    runtime: ActorRuntime,
    executable_environment: &str,
    default_executable: &str,
    expected_transport: &str,
    reply_text: &str,
) {
    let launcher = std::env::var_os("CCCC_LAUNCHER_PATH")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .expect("set CCCC_LAUNCHER_PATH to the current built cccc binary");
    assert_eq!(
        launcher.file_stem().and_then(|value| value.to_str()),
        Some("cccc")
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let daemon = tokio::spawn(crate::server::run(home.clone()));
    let client = cccc_client::DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store
        .create(&format!("{runtime:?} managed Actor busy canary"), "")
        .expect("group");
    let group_id = group.group_id.clone();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("project");
    store
        .mutate(&group_id, |group| {
            group.scopes.push(Scope {
                scope_key: "s_project".into(),
                url: root.to_string_lossy().into_owned(),
                label: "project".into(),
                git_remote: String::new(),
            });
            group.active_scope_key = "s_project".into();
            let mut actor = Actor::new(format!("{default_executable}-live"));
            actor.role = Some(ActorRole::Foreman);
            actor.runtime = runtime;
            actor.runner = RunnerKind::Headless;
            actor.command = vec![std::env::var_os(executable_environment).map_or_else(
                || default_executable.into(),
                |path| path.to_string_lossy().into_owned(),
            )];
            actors::add(group, actor)?;
            group.running = true;
            Ok(())
        })
        .expect("configure group");
    let group = store.load(&group_id).expect("group");
    let actor = group.actors[0].clone();

    let start = client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: "actor_start".into(),
            args: serde_json::Map::from_iter([
                ("group_id".into(), Value::String(group_id.clone())),
                ("actor_id".into(), Value::String(actor.id.clone())),
                ("by".into(), Value::String("user".into())),
            ]),
        })
        .await
        .expect("start managed Group Actor through daemon IPC");
    assert!(start.ok, "managed Actor start failed: {:?}", start.error);

    let elapsed = tokio::task::block_in_place(|| {
        assert_managed_actor_starts_idle(&store, &group, &actor)?;
        run_busy_actor_delivery_canary(&home, &store, &group, &actor, reply_text)
    })
    .expect("managed Actor busy-state delivery canary");
    eprintln!("{runtime:?} busy-state delivery reached the native TUI in {elapsed:?}");

    let receipt = cccc_core::fs::read_json::<Value>(
        &store
            .state_dir(&group_id)
            .expect("state dir")
            .join(format!("runtime_sessions/{}.json", actor.id)),
    )
    .expect("managed-session receipt");
    assert_eq!(receipt["transport"], expected_transport);
    assert_eq!(receipt["status"], "usable");

    let stopped_group_id = group_id.clone();
    let stopped_actor_id = actor.id.clone();
    tokio::task::spawn_blocking(move || {
        let _ = super::super::super::local_headless::stop(&stopped_group_id, &stopped_actor_id);
    })
    .await
    .expect("stop managed Group Actor");
    let shutdown = client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: serde_json::Map::new(),
        })
        .await
        .expect("shutdown daemon");
    assert!(shutdown.ok, "daemon shutdown failed: {:?}", shutdown.error);
    tokio::time::timeout(Duration::from_secs(10), daemon)
        .await
        .expect("daemon exit timeout")
        .expect("daemon task")
        .expect("daemon exit");
}
