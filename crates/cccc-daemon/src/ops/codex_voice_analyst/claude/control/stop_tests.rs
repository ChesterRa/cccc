use super::{Endpoint, list};
use crate::ops::codex_voice_analyst::{AnalystSession, LaunchConfig};
use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde_json::json;
use std::time::Duration;

/// Exercise the real CLI and its real background job, without an API request.
/// Moving the bound socket leaves its listener and the managed job alive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_claude_empty_session_resumes_without_a_prompt_after_control_gap() {
    if std::env::var("CCCC_CLAUDE_EMPTY_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("project");
    let config_dir = temp.path().join("claude");
    std::fs::create_dir(&config_dir).expect("isolated CLI config");
    cccc_core::fs::write_json(
        &config_dir.join(".claude.json"),
        &json!({"bypassPermissionsModeAccepted":true,"hasCompletedOnboarding":true}),
    )
    .expect("isolated CLI onboarding");
    cccc_core::fs::write_json(
        &config_dir.join("settings.json"),
        &json!({"skipDangerousModePermissionPrompt":true}),
    )
    .expect("isolated CLI settings");
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
    let mut session = AnalystSession::launch(&home, config)
        .await
        .expect("launch real background job");
    let endpoint = Endpoint::resolve(&config_dir).expect("resolve real endpoint");
    let before = list(&endpoint).await.expect("capture live jobs before gap");
    let job = before
        .iter()
        .find(|job| job["sessionId"].as_str() == Some(session.thread_id()))
        .expect("owned session is listed");
    let short = job["short"]
        .as_str()
        .expect("owned job identity")
        .to_owned();
    let marker = temp.path().join("owned-resource");
    std::fs::write(&marker, []).expect("owned resource");
    session.cleanup_paths.push(marker.clone());

    let socket = std::path::PathBuf::from(&endpoint.address);
    let hidden = socket.with_extension("hidden");
    std::fs::rename(&socket, &hidden).expect("hide bound endpoint");
    let started = std::time::Instant::now();
    let restore = async {
        // Longer than the former two-second absence confirmation window.
        tokio::time::sleep(Duration::from_secs(4)).await;
        std::fs::rename(&hidden, &socket).expect("restore same bound endpoint");
        started.elapsed()
    };
    let (stopped, gap) = tokio::join!(session.stop(session.generation()), restore);
    let after = list(&endpoint)
        .await
        .expect("capture live jobs after return");
    let still_listed = after.iter().any(|job| {
        job["short"].as_str() == Some(&short)
            && job["sessionId"].as_str() == Some(session.thread_id())
    });
    let ownership_retained = session.process_running() && marker.exists();
    eprintln!(
        "real endpoint-gap capture: gap_ms={}, stop_success={}, same_job_listed={}, ownership_retained={}",
        gap.as_millis(),
        stopped.is_ok(),
        still_listed,
        ownership_retained,
    );

    // Clean up the actual job even on the regression's failing implementation.
    session
        .stop(session.generation())
        .await
        .expect("confirm actual job removal after endpoint recovery");
    let final_jobs = list(&endpoint).await.expect("capture confirmed removal");
    assert!(!final_jobs.iter().any(|job| job["short"] == short));
    assert!(!marker.exists(), "confirmed stop cleans owned resources");

    assert!(gap >= Duration::from_secs(4));
    assert!(still_listed, "the same real job survived the endpoint gap");
    let error = stopped.expect_err(
        "a still-running task whose endpoint disappears and returns must not be reported stopped",
    );
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("stop unconfirmed"), "{error}");
    assert!(ownership_retained, "unconfirmed stop must retain ownership");
}
