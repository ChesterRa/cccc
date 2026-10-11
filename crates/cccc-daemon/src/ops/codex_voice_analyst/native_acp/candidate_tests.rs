use super::*;
use crate::ops::codex_voice_analyst::{
    ActorLaunchConfig, AnalystEvent, AnalystSession, LaunchConfig,
};
use cccc_contracts::RuntimeMode;
use cccc_core::HomeLayout;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
use tokio::sync::broadcast;

fn fixture(runtime: ActorRuntime) -> (tempfile::TempDir, HomeLayout, String) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    home.initialize().expect("initialize");
    let program = temp.path().join(name(runtime));
    std::fs::write(&program, include_str!("candidate_fixture.py")).expect("script");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("mode");
    (temp, home, program.to_string_lossy().into_owned())
}

fn environment(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "HOME".into(),
            root.join("provider").to_string_lossy().into_owned(),
        ),
        (
            "KIMI_CODE_HOME".into(),
            root.join("provider/kimi").to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            root.join("provider/config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            root.join("provider/data").to_string_lossy().into_owned(),
        ),
        ("ACP_FIXTURE_CLI".into(), "/bin/true".into()),
    ])
}

fn requests(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("candidate_requests.jsonl"))
        .expect("requests")
        .lines()
        .map(|line| serde_json::from_str(line).expect("frame"))
        .collect()
}

async fn terminal(events: &mut broadcast::Receiver<AnalystEvent>) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.message["method"] == "turn/completed" {
                return event.message;
            }
        }
    })
    .await
    .expect("completion")
}

#[test]
fn kimi_native_policy_is_preserved_without_spelling_based_escalation() {
    let safe = options(ActorRuntime::Kimi, &[]).expect("default");
    assert_eq!(safe.command, ["kimi", "acp"]);
    assert_eq!(safe.mode.as_deref(), Some("auto"));
    assert!(
        !safe.unrestricted,
        "native --yolo still asks for risky actions"
    );
    let unrestricted =
        options(ActorRuntime::Kimi, &["kimi".into(), "--auto".into()]).expect("Never Ask");
    assert_eq!(unrestricted.mode.as_deref(), Some("yolo"));
    assert!(unrestricted.unrestricted);
    let plan = options(ActorRuntime::Kimi, &["kimi".into(), "--plan".into()]).expect("plan");
    assert_eq!(plan.mode.as_deref(), Some("plan"));
    assert!(!plan.unrestricted);
    let ask = options(ActorRuntime::Auggie, &["auggie".into(), "-a".into()]).expect("ask");
    assert_eq!(ask.command, ["auggie", "--acp"]);
    assert_eq!(ask.mode.as_deref(), Some("ask"));
    assert!(!ask.unrestricted);
    for runtime in [ActorRuntime::Kimi, ActorRuntime::Auggie] {
        for flag in [
            "--resume",
            "--prompt",
            "--mcp-config",
            "--workspace-root",
            "--allow-indexing",
        ] {
            assert!(
                options(
                    runtime,
                    &[name(runtime).into(), flag.into(), "value".into()]
                )
                .is_err()
            );
        }
        assert!(options(runtime, &["sh".into(), name(runtime).into()]).is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn candidate_models_and_modes_are_reapplied_after_new_and_load() {
    for (runtime, flag, mode, method, field, model) in [
        (
            ActorRuntime::Kimi,
            "--yolo",
            "auto",
            "session/set_config_option",
            "value",
            "model-kimi",
        ),
        (
            ActorRuntime::Auggie,
            "--ask",
            "ask",
            "session/set_model",
            "modelId",
            "model-auggie",
        ),
    ] {
        let (temp, home, program) = fixture(runtime);
        let mut config = LaunchConfig::new(temp.path());
        config.runtime = runtime;
        config.runtime_mode = RuntimeMode::Acp;
        config.command = vec![program, "-m".into(), "Fixture model".into(), flag.into()];
        config.environment = environment(temp.path());
        let session = AnalystSession::launch(&home, config.clone())
            .await
            .expect("new");
        let id = session.thread_id().to_owned();
        cccc_core::fs::write_json(
            &home.daemon_dir().join("codex_voice_analyst.json"),
            &json!({"thread_id":id,"materialized":false}),
        )
        .expect("Voice receipt");
        let mut events = session.subscribe();
        session
            .start_turn(session.generation(), "initial", "fixture")
            .await
            .expect("turn");
        assert_eq!(
            terminal(&mut events).await["params"]["turn"]["status"],
            "completed"
        );
        session.stop(session.generation()).await.expect("stop");
        config.resume_thread_id = Some(id.clone());
        let loaded = AnalystSession::launch(&home, config.clone())
            .await
            .expect("load");
        assert_eq!(loaded.thread_id(), id);
        loaded.stop(loaded.generation()).await.expect("stop");
        let frames = requests(temp.path());
        let selections: Vec<_> = frames
            .iter()
            .filter(|frame| frame["method"] == method)
            .collect();
        assert_eq!(selections.len(), 2);
        assert!(
            selections
                .iter()
                .all(|frame| frame["params"][field] == model)
        );
        let modes: Vec<_> = frames
            .iter()
            .filter(|frame| frame["method"] == "session/set_mode")
            .collect();
        assert_eq!(modes.len(), 2);
        assert!(modes.iter().all(|frame| frame["params"]["modeId"] == mode));
        if runtime == ActorRuntime::Auggie {
            let consent: Vec<_> = frames
                .iter()
                .filter(|frame| frame["id"] == "index-startup")
                .collect();
            assert_eq!(consent.len(), 2);
            assert!(
                consent
                    .iter()
                    .all(|frame| frame["result"]["outcome"]["outcome"] == "cancelled")
            );
        }
        config.command[2] = "unavailable".into();
        assert!(AnalystSession::launch(&home, config).await.is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn candidate_actor_receipts_keep_mcp_identity_and_reject_lost_attempted_sessions() {
    for runtime in [ActorRuntime::Kimi, ActorRuntime::Auggie] {
        let (temp, home, program) = fixture(runtime);
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("fixture", "")
            .expect("group");
        let launch = |actor: &str| ActorLaunchConfig {
            workdir: temp.path().into(),
            group_id: group.group_id.clone(),
            actor_id: actor.into(),
            runtime,
            runtime_mode: RuntimeMode::Acp,
            command: vec![program.clone()],
            environment: environment(temp.path()),
        };
        let empty = AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .expect("empty");
        let empty_id = empty.thread_id().to_owned();
        empty.stop(empty.generation()).await.expect("stop");
        let first = AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .expect("new empty replacement");
        assert_ne!(first.thread_id(), empty_id);
        assert!(first.structured_only());
        assert!(!first.tui_ready());
        let mut events = first.subscribe();
        first
            .start_turn(first.generation(), "delivery", "fixture")
            .await
            .expect("turn");
        terminal(&mut events).await;
        let id = first.thread_id().to_owned();
        first.stop(first.generation()).await.expect("stop");
        let loaded = AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .expect("load attempted");
        assert_eq!(loaded.thread_id(), id);
        assert!(loaded.thread_resumed);
        loaded.stop(loaded.generation()).await.expect("stop");
        let second = AnalystSession::launch_actor(&home, launch("beta"))
            .await
            .expect("separate actor");
        assert_ne!(second.thread_id(), id);
        second.stop(second.generation()).await.expect("stop");
        std::fs::write(temp.path().join("candidate_reject_load"), "").expect("unavailable session");
        assert!(
            AnalystSession::launch_actor(&home, launch("alpha"))
                .await
                .is_err()
        );
        let frames = requests(temp.path());
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame["method"] == "session/new")
                .count(),
            3,
            "no silent replacement of attempted work"
        );
        for actor in ["alpha", "beta"] {
            assert!(frames.iter().any(|frame| {
                frame["params"]["mcpServers"][0]["env"]
                    .as_array()
                    .is_some_and(|env| {
                        env.iter().any(|entry| {
                            entry["name"] == "CCCC_ACTOR_ID" && entry["value"] == actor
                        }) && env.iter().any(|entry| {
                            entry["name"] == "CCCC_GROUP_ID" && entry["value"] == group.group_id
                        })
                    })
            }));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn candidate_auggie_late_indexing_permission_remains_explicit_and_cancellable() {
    let (temp, home, program) = fixture(ActorRuntime::Auggie);
    std::fs::write(temp.path().join("candidate_late_index"), "").expect("late index fixture");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("fixture", "")
        .expect("group");
    let session = AnalystSession::launch_actor(
        &home,
        ActorLaunchConfig {
            workdir: temp.path().into(),
            group_id: group.group_id,
            actor_id: "alpha".into(),
            runtime: ActorRuntime::Auggie,
            runtime_mode: RuntimeMode::Acp,
            command: vec![program],
            environment: environment(temp.path()),
        },
    )
    .await
    .expect("launch");
    let mut events = session.subscribe();
    session
        .start_turn(session.generation(), "first", "LATE_INDEX")
        .await
        .expect("admission");
    let request = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Some(request) = session.permissions().first() {
                return request.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("index consent");
    assert_eq!(request["title"], "Index workspace");
    let frames = requests(temp.path());
    assert!(
        !frames
            .iter()
            .any(|frame| frame["id"] == "candidate-late-index" && frame.get("result").is_some()),
        "index upload has no automatic approval"
    );
    while let Ok(event) = events.try_recv() {
        assert_ne!(
            event.message["method"], "turn/completed",
            "a consent request is not completion"
        );
    }
    session
        .interrupt(session.generation(), "ignored-by-acp")
        .await
        .expect("cancel");
    assert_eq!(
        terminal(&mut events).await["params"]["turn"]["status"],
        "cancelled"
    );
    assert!(session.permissions().is_empty());
    let frames = requests(temp.path());
    assert!(
        frames
            .iter()
            .any(|frame| frame["id"] == "candidate-late-index"
                && frame["result"]["outcome"]["outcome"] == "cancelled")
    );
    session
        .start_turn(session.generation(), "second", "fixture after cancellation")
        .await
        .expect("next turn");
    assert_eq!(
        terminal(&mut events).await["params"]["turn"]["status"],
        "completed"
    );
    session.stop(session.generation()).await.expect("stop");
}
