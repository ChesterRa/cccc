use super::super::*;
use super::live_support::{
    wait_for_interruptible_activity, wait_for_turn_status, wait_for_turn_text,
};
use cccc_core::HomeLayout;

/// Offline real-CLI probe: no credentials, model request, or synthetic prompt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_codex_empty_actor_and_analyst_resume_with_native_terminal() {
    use futures_util::FutureExt as _;
    use serde_json::json;
    use std::time::Duration;

    if std::env::var("CCCC_CODEX_EMPTY_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("project");
    let codex_home = temp.path().join("codex");
    std::fs::create_dir_all(&root).expect("project");
    std::fs::create_dir_all(&codex_home).expect("Codex home");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("Empty Codex startup probe", "")
        .expect("group");
    let environment = BTreeMap::from([
        (
            "CODEX_HOME".into(),
            codex_home.to_string_lossy().into_owned(),
        ),
        ("TERM".into(), "xterm-256color".into()),
    ]);
    let command = vec![
        std::env::var("CCCC_CODEX_EXECUTABLE").unwrap_or_else(|_| "codex".into()),
        "-c".into(),
        "model_provider=\"offline-probe\"".into(),
        "-c".into(),
        "model_providers.offline-probe.name=\"Offline probe\"".into(),
        "-c".into(),
        "model_providers.offline-probe.base_url=\"http://127.0.0.1:9\"".into(),
        "-c".into(),
        "model_providers.offline-probe.wire_api=\"responses\"".into(),
    ];
    for purpose in [SessionPurpose::Actor, SessionPurpose::VoiceAnalyst] {
        let mut thread_id = None;
        for cycle in 0..3 {
            let session = match purpose {
                SessionPurpose::Actor => {
                    AnalystSession::launch_actor(
                        &home,
                        ActorLaunchConfig {
                            workdir: root.clone(),
                            group_id: group.group_id.clone(),
                            actor_id: "empty-codex".into(),
                            runtime: ActorRuntime::Codex,
                            command: command.clone(),
                            environment: environment.clone(),
                        },
                    )
                    .await
                }
                SessionPurpose::VoiceAnalyst => {
                    AnalystSession::launch(
                        &home,
                        LaunchConfig {
                            workdir: root.clone(),
                            runtime: ActorRuntime::Codex,
                            command: command.clone(),
                            environment: environment.clone(),
                            resume_thread_id: thread_id.clone(),
                        },
                    )
                    .await
                }
            }
            .expect("launch empty managed session");
            let observed = std::panic::AssertUnwindSafe(async {
                if let Some(expected) = &thread_id {
                    assert_eq!(session.thread_id(), expected);
                    assert!(session.thread_resumed);
                }
                let ManagedProtocol::Codex(protocol) = &session.protocol else {
                    unreachable!()
                };
                let read = protocol
                    .request(
                        "thread/read",
                        json!({"threadId":session.thread_id(),"includeTurns":true}),
                        Duration::from_secs(5),
                    )
                    .await?;
                assert_eq!(read["thread"]["id"], session.thread_id());
                assert_eq!(
                    read["thread"]["turns"],
                    json!([]),
                    "startup must not run a model"
                );
                assert!(
                    std::path::Path::new(read["thread"]["path"].as_str().expect("rollout path"))
                        .is_file()
                );
                if cycle == 0 {
                    protocol
                        .request(
                            "thread/name/set",
                            json!({"threadId":session.thread_id(),"name":"User chosen name"}),
                            Duration::from_secs(5),
                        )
                        .await?;
                } else {
                    assert_eq!(
                        read["thread"]["name"], "User chosen name",
                        "resume must not rename history"
                    );
                }
                cccc_runtime::start(cccc_runtime::LaunchSpec {
                    group_id: group.group_id.clone(),
                    actor_id: "empty-codex".into(),
                    runner: cccc_contracts::RunnerKind::Pty,
                    command: session.actor_tui_command(),
                    cwd: root.clone(),
                    env: session.tui_environment(),
                    cols: 120,
                    rows: 40,
                })
                .map_err(io::Error::other)?;
                let ready = tokio::task::block_in_place(|| {
                    cccc_runtime::wait_for_input_ready(
                        &group.group_id,
                        "empty-codex",
                        Duration::from_secs(10),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                })
                .map_err(io::Error::other)?;
                assert!(ready, "native terminal did not initialize");
                tokio::time::sleep(Duration::from_secs(1)).await;
                let output = cccc_runtime::retained_history(&group.group_id, "empty-codex")
                    .map_err(io::Error::other)?
                    .data;
                assert!(!output.contains("Failed to resume session"), "{output}");
                assert!(!output.contains("no rollout found"), "{output}");
                assert!(
                    cccc_runtime::status(&group.group_id, "empty-codex")
                        .map_err(io::Error::other)?
                        .running
                );
                let read = protocol
                    .request(
                        "thread/read",
                        json!({"threadId":session.thread_id(),"includeTurns":true}),
                        Duration::from_secs(5),
                    )
                    .await?;
                assert_eq!(read["thread"]["turns"], json!([]));
                Ok::<(), io::Error>(())
            })
            .catch_unwind()
            .await;
            thread_id = Some(session.thread_id().to_owned());
            let _ = cccc_runtime::stop(&group.group_id, "empty-codex");
            session
                .stop(session.generation())
                .await
                .expect("stop empty managed session");
            observed
                .expect("startup probe assertion")
                .expect("durable empty session and native attach");
        }
    }
}

fn live_launch_config(root: &std::path::Path) -> LaunchConfig {
    let mut config = LaunchConfig::new(root);
    let model = std::env::var("CCCC_VOICE_ANALYST_MODEL").unwrap_or_else(|_| "gpt-5.6-sol".into());
    config.command = vec![
        std::env::var_os("CCCC_CODEX_EXECUTABLE").map_or_else(
            || "codex".into(),
            |path| path.to_string_lossy().into_owned(),
        ),
        "--model".into(),
        model,
    ];
    config
}

#[tokio::test]
async fn live_codex_app_server_session_when_explicitly_enabled() {
    if std::env::var("CCCC_VOICE_ANALYST_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join("banana"), "TOPAZ\n").expect("fixture");
    let config = live_launch_config(&root);
    let session = AnalystSession::launch(&home, config)
        .await
        .expect("launch live Analyst");
    assert_eq!(session.binding().root, root.canonicalize().expect("root"));
    assert!(super::super::process::validate_loopback_endpoint(session.endpoint()).is_ok());
    assert!(!session.thread_id().is_empty());
    let tui_command = session.tui_command();
    let remote = tui_command
        .iter()
        .position(|argument| argument == "--remote")
        .expect("remote TUI endpoint flag");
    assert_eq!(
        tui_command.get(remote + 1).map(String::as_str),
        Some(session.endpoint())
    );
    let resume = tui_command
        .iter()
        .position(|argument| argument == "resume")
        .expect("remote TUI resume command");
    assert_eq!(
        tui_command.get(resume + 1).map(String::as_str),
        Some(session.thread_id())
    );
    assert!(tui_command.windows(2).any(|pair| pair[0] == "--model"));

    let mut events = session.subscribe();
    let generation = session.generation().to_owned();
    let turn = session
        .start_turn(
            &generation,
            "live-read-banana",
            "Read the file named banana in the current repository. Reply with only its exact content.",
        )
        .await
        .expect("start live turn");
    assert_eq!(
        wait_for_turn_text(&mut events, &turn.turn_id).await.trim(),
        "TOPAZ"
    );

    let steered = session
        .start_turn(
            &generation,
            "live-steer",
            "Inspect the repository carefully and eventually reply with only ORIGINAL.",
        )
        .await
        .expect("start steer turn");
    session
        .steer(
            &generation,
            &steered.turn_id,
            "Correction: do not return ORIGINAL. Reply with only STEERED_TOPAZ.",
        )
        .await
        .expect("steer live turn");
    assert_eq!(
        wait_for_turn_text(&mut events, &steered.turn_id)
            .await
            .trim(),
        "STEERED_TOPAZ"
    );

    let cancelled = session
        .start_turn(
            &generation,
            "live-cancel",
            "Run the shell command `sleep 30` before answering LATE_RESULT.",
        )
        .await
        .expect("start cancel turn");
    wait_for_interruptible_activity(&mut events, &cancelled.turn_id).await;
    session
        .interrupt(&generation, &cancelled.turn_id)
        .await
        .expect("interrupt live turn");
    assert_eq!(
        wait_for_turn_status(&mut events, &cancelled.turn_id).await,
        "interrupted"
    );

    let thread_id = session.thread_id().to_owned();
    let session = session
        .reconnect(&generation)
        .await
        .expect("reconnect live controller");
    let reconnect_generation = session.generation().to_owned();
    assert_ne!(reconnect_generation, generation);
    assert_eq!(session.thread_id(), thread_id);
    assert_eq!(
        session
            .start_turn(
                &reconnect_generation,
                "live-read-banana",
                "must return the existing delegation receipt",
            )
            .await
            .expect("deduplicated reconnect receipt"),
        turn
    );
    session
        .stop(&reconnect_generation)
        .await
        .expect("stop live Analyst");

    let mut resume_config = live_launch_config(&root);
    resume_config.resume_thread_id = Some(thread_id.clone());
    let resumed = AnalystSession::launch(&home, resume_config)
        .await
        .expect("resume Analyst in a new app-server process");
    assert_eq!(resumed.thread_id(), thread_id);
    let resumed_generation = resumed.generation().to_owned();
    let mut resumed_events = resumed.subscribe();
    let continuity = resumed
        .start_turn(
            &resumed_generation,
            "live-resume-history",
            "From the earlier conversation, reply with only the exact content read from the file named banana. Do not add punctuation.",
        )
        .await
        .expect("resume history turn");
    assert_eq!(
        wait_for_turn_text(&mut resumed_events, &continuity.turn_id)
            .await
            .trim(),
        "TOPAZ"
    );
    resumed
        .stop(&resumed_generation)
        .await
        .expect("stop resumed Analyst");
}
