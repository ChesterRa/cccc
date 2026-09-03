use super::super::*;
use super::live_support::{wait_for_turn_status, wait_for_turn_text};
use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use std::time::Duration;

const CANCELLATION_READY_TIMEOUT: Duration = Duration::from_secs(60);

fn live_launch_config(root: &std::path::Path) -> LaunchConfig {
    let mut config = LaunchConfig::new(root);
    config.runtime = ActorRuntime::Opencode;
    config.command = vec![std::env::var_os("CCCC_OPENCODE_EXECUTABLE").map_or_else(
        || "opencode".into(),
        |path| path.to_string_lossy().into_owned(),
    )];
    config
}

#[tokio::test]
async fn live_opencode_managed_session_when_explicitly_enabled() {
    if std::env::var("CCCC_OPENCODE_MANAGED_LIVE").as_deref() != Ok("1") {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join("managed-marker.txt"), "TOPAZ_OPENCODE_MANAGED\n").expect("fixture");

    let session = AnalystSession::launch(&home, live_launch_config(&root))
        .await
        .expect("launch live OpenCode Analyst");
    let generation = session.generation().to_owned();
    let session_id = session.thread_id().to_owned();
    assert!(!session_id.is_empty());
    let tui = session.tui_command();
    assert!(
        tui.windows(2)
            .any(|parts| parts == ["--session", session_id.as_str()])
    );
    assert!(tui.iter().any(|argument| argument == "attach"));
    let mut events = session.subscribe();
    let turn = session
        .start_turn(
            &generation,
            "opencode-live-read",
            "Read managed-marker.txt and reply with only its exact content.",
        )
        .await
        .expect("start live OpenCode turn");
    assert!(
        wait_for_turn_text(&mut events, &turn.turn_id)
            .await
            .contains("TOPAZ_OPENCODE_MANAGED")
    );

    let cancel_ready = root.join("cancel-ready.txt");
    let cancelled = session
        .start_turn(
            &generation,
            "opencode-live-cancel",
            "Use the shell tool to run exactly `printf READY > cancel-ready.txt; sleep 60`, then reply with LATE_OPENCODE_RESULT.",
        )
        .await
        .expect("start live cancellation turn");
    tokio::time::timeout(CANCELLATION_READY_TIMEOUT, async {
        while !cancel_ready.is_file() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("OpenCode cancellation fixture did not enter shell execution");
    session
        .interrupt(&generation, &cancelled.turn_id)
        .await
        .expect("cancel live OpenCode turn");
    assert_eq!(
        wait_for_turn_status(&mut events, &cancelled.turn_id).await,
        "cancelled"
    );
    session.stop(&generation).await.expect("stop live OpenCode");

    let mut resume = live_launch_config(&root);
    resume.resume_thread_id = Some(session_id.clone());
    let resumed = AnalystSession::launch(&home, resume)
        .await
        .expect("resume live OpenCode Analyst");
    assert_eq!(resumed.thread_id(), session_id);
    let resumed_generation = resumed.generation().to_owned();
    let mut resumed_events = resumed.subscribe();
    let continuity = resumed
        .start_turn(
            &resumed_generation,
            "opencode-live-resume",
            "What exact marker did you read earlier? Reply with only that marker.",
        )
        .await
        .expect("start resumed OpenCode turn");
    assert!(
        wait_for_turn_text(&mut resumed_events, &continuity.turn_id)
            .await
            .contains("TOPAZ_OPENCODE_MANAGED")
    );
    resumed
        .stop(&resumed_generation)
        .await
        .expect("stop resumed OpenCode");
}
