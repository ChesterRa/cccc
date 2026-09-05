use super::super::*;
use cccc_contracts::ActorRuntime;
use cccc_core::{HomeLayout, profiles::ProfileStore};
use std::io;
use std::path::Path;
use std::time::Duration;
use tokio::sync::broadcast;

fn configure_isolated_claude(config_dir: &Path) {
    std::fs::create_dir_all(config_dir).expect("isolated Claude config");
    cccc_core::fs::write_json(
        &config_dir.join(".claude.json"),
        &serde_json::json!({"bypassPermissionsModeAccepted":true,"hasCompletedOnboarding":true}),
    )
    .expect("isolated CLI onboarding");
    cccc_core::fs::write_json(
        &config_dir.join("settings.json"),
        &serde_json::json!({"skipDangerousModePermissionPrompt":true}),
    )
    .expect("isolated CLI settings");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_claude_cold_terminal_preserves_large_first_delivery() {
    if std::env::var("CCCC_CLAUDE_TERMINAL_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("project");
    let config_dir = temp.path().join("claude");
    configure_isolated_claude(&config_dir);
    // PR #97: this fresh directory has never been trusted. The managed
    // background session and its attach must still capture the first input.
    let before: serde_json::Value =
        cccc_core::fs::read_json(&config_dir.join(".claude.json")).expect("config");
    assert!(before.get("projects").is_none());
    let mut config = LaunchConfig::new(&root);
    config.runtime = ActorRuntime::Claude;
    config.environment.extend([
        (
            "CLAUDE_CONFIG_DIR".into(),
            config_dir.to_string_lossy().into_owned(),
        ),
        // This probe checks input capture only; no real model or credentials.
        ("ANTHROPIC_BASE_URL".into(), "http://127.0.0.1:9".into()),
        (
            "ANTHROPIC_AUTH_TOKEN".into(),
            "cccc-local-input-test".into(),
        ),
        ("API_TIMEOUT_MS".into(), "1000".into()),
    ]);
    let session = AnalystSession::launch(&home, config).await.expect("launch");
    let group_id = uuid::Uuid::new_v4().simple().to_string();
    let mut actor = cccc_contracts::Actor::new("cold-claude");
    actor.runtime = ActorRuntime::Claude;
    let text = format!(
        "Startup context.\n{}\nTAIL_COMPLETE_7e42",
        "x".repeat(16_038)
    );
    let observed = async {
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor.id.clone(),
            runner: cccc_contracts::RunnerKind::Pty,
            command: session.actor_tui_command(),
            cwd: root.clone(),
            env: session.tui_environment(),
            cols: 120,
            rows: 40,
        })
        .map_err(io::Error::other)?;
        let accepted = tokio::task::spawn_blocking({
            let group_id = group_id.clone();
            let actor = actor.clone();
            let text = text.clone();
            move || {
                crate::ops::actor_delivery::submit_terminal_text(
                    &group_id,
                    &actor,
                    &text,
                    &std::sync::atomic::AtomicBool::new(false),
                )
            }
        })
        .await
        .map_err(io::Error::other)?;
        if !accepted {
            return Err(io::Error::other("native delivery rejected"));
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                // Agent View may not publish linkScanPath until its first
                // terminal result. Read this isolated session's durable input
                // directly while the intentionally offline provider retries.
                let projects = match std::fs::read_dir(config_dir.join("projects")) {
                    Ok(projects) => projects,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                for project in projects {
                    let path = project?
                        .path()
                        .join(format!("{}.jsonl", session.thread_id()));
                    if let Ok(content) = std::fs::read_to_string(path) {
                        for line in content.lines() {
                            if let Ok(record) = serde_json::from_str::<serde_json::Value>(line)
                                && record["type"] == "user"
                                && let Some(content) = record
                                    .pointer("/message/content")
                                    .and_then(serde_json::Value::as_str)
                            {
                                return Ok::<_, io::Error>(content.to_owned());
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(io::Error::other)?
    }
    .await;
    let stopped = session.stop(session.generation()).await;
    let terminal_stopped = cccc_runtime::stop(&group_id, &actor.id);
    stopped.expect("stop owned Claude job");
    terminal_stopped.expect("stop owned terminal");
    assert_eq!(observed.expect("captured input"), text);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_claude_requires_bypass_ack_before_creating_a_managed_session() {
    if std::env::var("CCCC_CLAUDE_TERMINAL_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let root = temp.path().join("project");
    let config_dir = temp.path().join("claude");
    std::fs::create_dir_all(&root).expect("project");
    std::fs::create_dir_all(&config_dir).expect("config directory");
    cccc_core::fs::write_json(
        &config_dir.join(".claude.json"),
        &serde_json::json!({"hasCompletedOnboarding":true}),
    )
    .expect("onboarding without trust or bypass acknowledgement");
    let mut config = LaunchConfig::new(&root);
    config.runtime = ActorRuntime::Claude;
    config.environment.extend([
        (
            "CLAUDE_CONFIG_DIR".into(),
            config_dir.to_string_lossy().into_owned(),
        ),
        ("ANTHROPIC_BASE_URL".into(), "http://127.0.0.1:9".into()),
        (
            "ANTHROPIC_AUTH_TOKEN".into(),
            "cccc-local-input-test".into(),
        ),
    ]);
    let error = match AnalystSession::launch(&home, config).await {
        Ok(session) => {
            session
                .stop(session.generation())
                .await
                .expect("stop unexpected job");
            panic!("unacknowledged bypass mode must not create a managed session");
        }
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("accepting the disclaimer first"),
        "{error}"
    );
    let jobs = config_dir.join("jobs");
    assert!(!jobs.exists() || std::fs::read_dir(jobs).expect("jobs").next().is_none());
    let config: serde_json::Value =
        cccc_core::fs::read_json(&config_dir.join(".claude.json")).expect("config");
    assert!(config.get("projects").is_none());
    assert_ne!(config["bypassPermissionsModeAccepted"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_claude_empty_session_resumes_without_a_prompt() {
    if std::env::var("CCCC_CLAUDE_EMPTY_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("project");
    let config_dir = temp.path().join("claude");
    configure_isolated_claude(&config_dir);
    let mut thread_id = None;
    for _ in 0..3 {
        let mut config = LaunchConfig::new(&root);
        config.runtime = ActorRuntime::Claude;
        config.environment.insert(
            "CLAUDE_CONFIG_DIR".into(),
            config_dir.to_string_lossy().into_owned(),
        );
        config.resume_thread_id = thread_id.clone();
        let session = AnalystSession::launch(&home, config)
            .await
            .expect("launch empty session");
        if let Some(expected) = &thread_id {
            assert_eq!(session.thread_id(), expected);
        }
        thread_id = Some(session.thread_id().to_owned());
        session
            .stop(session.generation())
            .await
            .expect("stop empty session");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_claude_voice_session_resumes_when_explicitly_enabled() {
    if std::env::var("CCCC_CLAUDE_VOICE_ANALYST_LIVE").as_deref() != Ok("1") {
        return;
    }
    let source_home = std::env::var_os("CCCC_CLAUDE_PROFILE_HOME")
        .map(HomeLayout::from_path)
        .transpose()
        .expect("source CCCC home")
        .expect("set CCCC_CLAUDE_PROFILE_HOME to a CCCC home containing the test profile");
    let profile_name = std::env::var("CCCC_CLAUDE_PROFILE_NAME")
        .expect("set CCCC_CLAUDE_PROFILE_NAME to the Claude live-test profile name");
    let store = ProfileStore::new(source_home).expect("profile store");
    let profile = store
        .list()
        .expect("list profiles")
        .into_iter()
        .find(|profile| profile["name"].as_str() == Some(profile_name.as_str()))
        .expect("Claude live-test profile");
    let id = profile["id"].as_str().expect("profile id");
    let scope = profile["scope"].as_str().unwrap_or("global");
    let owner = profile["owner_id"].as_str().unwrap_or_default();
    let runtime = store
        .runtime_ref(id, scope, owner)
        .expect("resolve profile")
        .expect("profile still exists");
    assert_eq!(runtime.runtime, ActorRuntime::Claude);

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("project");
    let marker = format!(
        "COBALT_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );

    let first = AnalystSession::launch(&home, launch_config(&root, &runtime, None))
        .await
        .expect("launch Claude Voice Analyst");
    let thread_id = first.thread_id().to_owned();
    let first_result = run_turn(
        &first,
        "claude-live-first",
        &format!("Remember the marker {marker}. Reply with only ACK."),
    )
    .await;
    let first_stop = first.stop(first.generation()).await;
    first_result.expect("first Claude turn");
    first_stop.expect("stop first Claude job");

    let resumed = AnalystSession::launch(
        &home,
        launch_config(&root, &runtime, Some(thread_id.clone())),
    )
    .await
    .expect("resume exact Claude Voice Analyst session");
    assert_eq!(resumed.thread_id(), thread_id);
    let resumed_result = run_turn(
        &resumed,
        "claude-live-resume",
        "Reply with only the marker I asked you to remember in the previous turn.",
    )
    .await;
    let resumed_stop = resumed.stop(resumed.generation()).await;
    let resumed_result = resumed_result.expect("resumed Claude turn");
    resumed_stop.expect("stop resumed Claude job");
    assert_eq!(resumed_result.trim(), marker);
}

fn launch_config(
    root: &Path,
    runtime: &cccc_core::profiles::RuntimeProfileConfig,
    resume_thread_id: Option<String>,
) -> LaunchConfig {
    let mut config = LaunchConfig::new(root);
    config.runtime = runtime.runtime;
    config.command = runtime.command.clone();
    config.environment = runtime.environment.clone();
    config.resume_thread_id = resume_thread_id;
    config
}

async fn run_turn(session: &AnalystSession, delegation_id: &str, text: &str) -> io::Result<String> {
    let mut events = session.subscribe();
    let receipt = session
        .start_turn(session.generation(), delegation_id, text)
        .await?;
    tokio::time::timeout(
        Duration::from_secs(180),
        wait_for_result(&mut events, &receipt.turn_id),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Claude live turn timed out"))?
}

async fn wait_for_result(
    events: &mut broadcast::Receiver<AnalystEvent>,
    turn_id: &str,
) -> io::Result<String> {
    let mut text = String::new();
    loop {
        let event = events.recv().await.map_err(io::Error::other)?;
        let method = event.message["method"].as_str().unwrap_or_default();
        let params = &event.message["params"];
        if method == MANAGED_AGENT_DISCONNECTED_METHOD {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                params["reason"]
                    .as_str()
                    .unwrap_or("Claude managed session disconnected"),
            ));
        }
        if method == "item/completed"
            && params["turnId"] == turn_id
            && params["item"]["type"] == "agentMessage"
        {
            text = params["item"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
        }
        if method == "turn/completed" && params["turn"]["id"] == turn_id {
            if params["turn"]["status"] == "completed" {
                return Ok(text);
            }
            return Err(io::Error::other(
                params["turn"]["error"]
                    .as_str()
                    .unwrap_or("Claude turn did not complete"),
            ));
        }
    }
}
