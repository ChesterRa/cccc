use super::super::*;
use cccc_core::HomeLayout;
use std::path::Path;
use std::time::Duration;

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
    let group_id = uuid::Uuid::new_v4().simple().to_string();
    for _ in 0..3 {
        let mut config = LaunchConfig::new(&root);
        config.runtime = ActorRuntime::Claude;
        config.environment.insert(
            "CLAUDE_CONFIG_DIR".into(),
            config_dir.to_string_lossy().into_owned(),
        );
        config.environment.extend([
            ("ANTHROPIC_BASE_URL".into(), "http://127.0.0.1:9".into()),
            (
                "ANTHROPIC_AUTH_TOKEN".into(),
                "cccc-local-input-test".into(),
            ),
        ]);
        config.resume_thread_id = thread_id.clone();
        let session = AnalystSession::launch(&home, config)
            .await
            .expect("launch empty session");
        if let Some(expected) = &thread_id {
            assert_eq!(session.thread_id(), expected);
        }
        thread_id = Some(session.thread_id().to_owned());
        let attached = cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group_id.clone(),
            actor_id: "empty-claude".into(),
            runner: cccc_contracts::RunnerKind::Pty,
            command: session.actor_tui_command(),
            cwd: root.clone(),
            env: session.tui_environment(),
            cols: 120,
            rows: 40,
        });
        let ready = if attached.is_ok() {
            tokio::task::block_in_place(|| {
                cccc_runtime::wait_for_input_ready(
                    &group_id,
                    "empty-claude",
                    Duration::from_secs(10),
                    &std::sync::atomic::AtomicBool::new(false),
                )
            })
        } else {
            Ok(false)
        };
        session
            .stop(session.generation())
            .await
            .expect("stop empty session");
        cccc_runtime::stop(&group_id, "empty-claude").expect("stop empty terminal");
        attached.expect("attach empty native terminal");
        assert!(ready.expect("terminal readiness"));
        // The real worker need not publish its zero counter before stopping.
        // Model that valid Agent View refresh in this isolated, stopped job so
        // every cold resume exercises tokens=0, not just provisional null.
        for job in std::fs::read_dir(config_dir.join("jobs")).expect("jobs") {
            let job = job.expect("job");
            if !job.file_type().expect("type").is_dir() {
                continue;
            }
            let path = job.path().join("state.json");
            let mut state: serde_json::Value = cccc_core::fs::read_json(&path).expect("state");
            if state["sessionId"].as_str() == Some(session.thread_id()) {
                assert!(state["tokens"].is_null() || state["tokens"] == 0);
                assert!(state["linkScanPath"].is_null());
                assert_eq!(state["intent"], "");
                state["tokens"] = serde_json::json!(0);
                cccc_core::fs::write_json(&path, &state).expect("zero-usage metadata");
            }
        }
    }
}
