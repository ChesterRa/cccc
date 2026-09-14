use super::super::*;
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
