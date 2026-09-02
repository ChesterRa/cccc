use super::super::*;
use super::live_support::{
    wait_for_interruptible_activity, wait_for_turn_status, wait_for_turn_text,
};
use cccc_core::HomeLayout;

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
