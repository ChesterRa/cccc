use cccc_contracts::{Actor, ActorRole, ActorRuntime, Event, RunnerKind};
use cccc_core::{GroupStore, HomeLayout, Scope, actors, ledger};
use serde_json::{Value, json};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use super::live_support::wait_for_daemon;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_opencode_group_actor_delivers_and_replies_when_explicitly_enabled() {
    if std::env::var("CCCC_OPENCODE_MANAGED_LIVE").as_deref() != Ok("1") {
        return;
    }
    let _launcher = std::env::var_os("CCCC_LAUNCHER_PATH")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .expect("set CCCC_LAUNCHER_PATH to the current built cccc binary");
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let daemon = tokio::spawn(crate::server::run(home.clone()));
    let client = cccc_client::DaemonClient::new(home.clone());
    wait_for_daemon(&client).await;
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store
        .create("OpenCode managed Actor canary", "")
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
            let mut actor = Actor::new("opencode-live");
            actor.role = Some(ActorRole::Foreman);
            actor.runtime = ActorRuntime::Opencode;
            actor.runner = RunnerKind::Headless;
            actor.command = vec![std::env::var_os("CCCC_OPENCODE_EXECUTABLE").map_or_else(
                || "opencode".into(),
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
        .expect("start OpenCode Group Actor through daemon IPC");
    assert!(start.ok, "OpenCode Actor start failed: {:?}", start.error);
    let outcome = tokio::task::block_in_place(|| run_canary(&home, &store, &group, &actor));
    let stopped_group_id = group_id.clone();
    let stopped_actor_id = actor.id.clone();
    tokio::task::spawn_blocking(move || {
        super::super::super::local_headless::stop(&stopped_group_id, &stopped_actor_id);
    })
    .await
    .expect("stop OpenCode Group Actor");
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
    outcome.expect("OpenCode Group Actor canary");
}

fn run_canary(
    home: &HomeLayout,
    store: &GroupStore,
    group: &cccc_core::GroupDoc,
    actor: &Actor,
) -> io::Result<()> {
    let event_path = store
        .state_dir(&group.group_id)?
        .join("headless/events.jsonl");
    wait_for(Duration::from_secs(180), "bootstrap completion", || {
        Ok(headless_events(&event_path).iter().any(|event| {
            event["actor_id"] == actor.id
                && event["type"] == "headless.control.completed"
                && event["data"]["control_kind"] == "bootstrap"
        }))
    })?;
    let mut source = Event::new("chat.message", &group.group_id);
    source.by = "user".into();
    source.data = json!({
        "text":"Reply with exactly OPENCODE_GROUP_REPLY using the required CCCC reply tool.",
        "to":[actor.id],
        "message_mode":"request_reply",
    })
    .as_object()
    .cloned()
    .expect("source data");
    ledger::append(&store.ledger_path(&group.group_id)?, &source)
        .map_err(|error| stage_error("append source event", error))?;
    if !super::super::super::local_headless::submit(home, group, actor, &source) {
        return Err(io::Error::other(
            "OpenCode Group Actor rejected the queued delivery",
        ));
    }
    wait_for(Duration::from_secs(180), "actor MCP reply", || {
        Ok(
            ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger path"))
                .unwrap_or_default()
                .iter()
                .any(|event| {
                    event.kind == "chat.message"
                        && event.by == actor.id
                        && event.data.get("reply_to").and_then(Value::as_str)
                            == Some(source.id.as_str())
                        && event.data.get("text").and_then(Value::as_str)
                            == Some("OPENCODE_GROUP_REPLY")
                }),
        )
    })?;
    let receipt = cccc_core::fs::read_json::<Value>(
        &store
            .state_dir(&group.group_id)?
            .join(format!("runtime_sessions/{}.json", actor.id)),
    )
    .map_err(|error| stage_error("read managed-session receipt", error))?;
    if receipt["transport"] != "opencode_acp_attach" || receipt["status"] != "usable" {
        return Err(io::Error::other(format!(
            "unexpected OpenCode managed receipt: {receipt}"
        )));
    }
    Ok(())
}

fn stage_error(stage: &str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{stage}: {error}"))
}

fn headless_events(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn wait_for(
    timeout: Duration,
    stage: &str,
    ready: impl Fn() -> io::Result<bool>,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if ready()? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("live OpenCode canary timed out waiting for {stage}"),
    ))
}
