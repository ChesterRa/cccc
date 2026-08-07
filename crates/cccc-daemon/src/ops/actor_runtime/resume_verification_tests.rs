use super::*;
use cccc_contracts::{ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, actors};
use cccc_runtime::LaunchSpec;
use serde_json::json;

#[test]
fn delayed_resume_rejection_starts_a_fresh_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let created = store.create("resume fallback", "").expect("group");
    let cwd = temp.path().join("repo");
    std::fs::create_dir(&cwd).expect("cwd");
    let fresh_command = vec!["sh".into(), "-c".into(), "sleep 5".into()];
    let actor = store
        .mutate(&created.group_id, |group| {
            let mut actor = Actor::new("peer1");
            actor.runtime = ActorRuntime::Custom;
            actor.runner = RunnerKind::Pty;
            actor.command = fresh_command.clone();
            actors::add(group, actor)
        })
        .expect("actor");
    let group = store.load(&created.group_id).expect("group");
    let session_dir = store
        .state_dir(&group.group_id)
        .expect("state dir")
        .join("runtime_sessions");
    std::fs::create_dir_all(&session_dir).expect("session dir");
    cccc_core::fs::write_json(
        &session_dir.join("peer1.json"),
        &json!({
            "runtime":"codex",
            "status":"usable",
            "resume_eligible":true,
            "failure_count":0
        }),
    )
    .expect("resume metadata");

    let resumed_status = cccc_runtime::start(LaunchSpec {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        runner: RunnerKind::Pty,
        command: vec![
            "sh".into(),
            "-c".into(),
            "sleep 0.15; printf 'ERROR: No saved session found with ID stale\\n'; sleep 5".into(),
        ],
        cwd: cwd.clone(),
        env: BTreeMap::new(),
        cols: 120,
        rows: 40,
    })
    .expect("resumed process");

    schedule_with_timing(
        home.clone(),
        group.clone(),
        actor.clone(),
        cwd,
        BTreeMap::new(),
        fresh_command,
        resumed_status.clone(),
        VerificationTiming {
            capture_delay: Duration::from_millis(50),
            monitor_duration: Duration::from_secs(1),
            poll_interval: Duration::from_millis(20),
        },
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    let fresh = loop {
        if let Ok(status) = cccc_runtime::status(&group.group_id, &actor.id)
            && status.running
            && status.started_at != resumed_status.started_at
        {
            break status;
        }
        assert!(Instant::now() < deadline, "fresh fallback did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    let stored: serde_json::Value =
        cccc_core::fs::read_json(&session_dir.join("peer1.json")).expect("stored metadata");
    assert_eq!(stored["status"], "resume_failed");
    assert_eq!(stored["resume_eligible"], false);
    assert_eq!(fresh.actor_id, "peer1");
    cccc_runtime::stop(&group.group_id, &actor.id).expect("stop fresh process");
}
