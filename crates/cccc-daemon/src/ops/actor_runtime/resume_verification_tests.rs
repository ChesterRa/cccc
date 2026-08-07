use super::*;
use cccc_contracts::{ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, actors};
use cccc_runtime::{HistoryConfig, LaunchSpec};
use serde_json::json;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    actor: Actor,
    cwd: PathBuf,
    fresh_command: Vec<String>,
    session_dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let created = store.create(name, "").expect("group");
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
        Self {
            _temp: temp,
            home,
            group,
            actor,
            cwd,
            fresh_command,
            session_dir,
        }
    }

    fn start_resumed(&self, command: &str) -> SessionStatus {
        cccc_runtime::start(LaunchSpec {
            group_id: self.group.group_id.clone(),
            actor_id: self.actor.id.clone(),
            runner: RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), command.into()],
            cwd: self.cwd.clone(),
            env: BTreeMap::new(),
            cols: 120,
            rows: 40,
        })
        .expect("resumed process")
    }

    fn schedule(&self, resumed_status: SessionStatus) {
        schedule_with_timing(
            self.home.clone(),
            self.group.clone(),
            self.actor.clone(),
            self.cwd.clone(),
            BTreeMap::new(),
            self.fresh_command.clone(),
            resumed_status,
            VerificationTiming {
                capture_delay: Duration::from_millis(50),
                monitor_duration: Duration::from_secs(1),
                poll_interval: Duration::from_millis(20),
            },
        );
    }
}

#[test]
fn delayed_resume_rejection_starts_a_fresh_process() {
    let fixture = Fixture::new("resume fallback");
    let resumed_status = fixture.start_resumed(
        "sleep 0.15; printf 'ERROR: No saved session found with ID stale\\n'; sleep 5",
    );
    fixture.schedule(resumed_status.clone());

    let deadline = Instant::now() + Duration::from_secs(3);
    let fresh = loop {
        if let Ok(status) = cccc_runtime::status(&fixture.group.group_id, &fixture.actor.id)
            && status.running
            && status.started_at != resumed_status.started_at
        {
            break status;
        }
        assert!(Instant::now() < deadline, "fresh fallback did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    let stored: serde_json::Value =
        cccc_core::fs::read_json(&fixture.session_dir.join("peer1.json")).expect("stored metadata");
    assert_eq!(stored["status"], "resume_failed");
    assert_eq!(stored["resume_eligible"], false);
    assert_eq!(fresh.actor_id, "peer1");
    cccc_runtime::stop(&fixture.group.group_id, &fixture.actor.id).expect("stop fresh process");
}

#[test]
fn explicit_stop_cancels_fresh_fallback() {
    let fixture = Fixture::new("resume stop");
    let resumed_status = fixture.start_resumed(
        "sleep 0.15; printf 'ERROR: No saved session found with ID stale\\n'; sleep 5",
    );
    fixture.schedule(resumed_status);

    super::super::stop(&fixture.group, &fixture.actor.id).expect("explicit stop");
    std::thread::sleep(Duration::from_millis(400));

    assert!(matches!(
        cccc_runtime::status(&fixture.group.group_id, &fixture.actor.id),
        Err(cccc_runtime::RuntimeError::NotFound(_, _))
    ));
    let stored: serde_json::Value =
        cccc_core::fs::read_json(&fixture.session_dir.join("peer1.json")).expect("stored metadata");
    assert_eq!(stored["status"], "usable");
    assert_eq!(stored["failure_count"], 0);
}

#[test]
fn conditional_rollback_cancels_fresh_fallback() {
    let fixture = Fixture::new("resume rollback");
    let resumed_status = fixture.start_resumed(
        "sleep 0.15; printf 'ERROR: No saved session found with ID stale\\n'; sleep 5",
    );
    fixture.schedule(resumed_status.clone());

    super::super::stop_if_started_at(&fixture.group, &resumed_status)
        .expect("conditional rollback");
    std::thread::sleep(Duration::from_millis(400));

    assert!(matches!(
        cccc_runtime::status(&fixture.group.group_id, &fixture.actor.id),
        Err(cccc_runtime::RuntimeError::NotFound(_, _))
    ));
    let stored: serde_json::Value =
        cccc_core::fs::read_json(&fixture.session_dir.join("peer1.json")).expect("stored metadata");
    assert_eq!(stored["status"], "usable");
    assert_eq!(stored["failure_count"], 0);
}

#[test]
fn daemon_shutdown_gate_does_not_invalidate_resume_metadata() {
    let fixture = Fixture::new("resume shutdown");
    let resumed_status = fixture.start_resumed(
        "sleep 0.15; printf 'ERROR: No saved session found with ID stale\\n'; sleep 5",
    );
    fixture.schedule(resumed_status);

    crate::runtime_start_gate::prevent(&fixture.home).expect("prevent starts");
    std::thread::sleep(Duration::from_millis(400));

    let stored: serde_json::Value =
        cccc_core::fs::read_json(&fixture.session_dir.join("peer1.json")).expect("stored metadata");
    assert_eq!(stored["status"], "usable");
    assert_eq!(stored["failure_count"], 0);
    crate::runtime_start_gate::allow(&fixture.home).expect("restore starts");
    cccc_runtime::stop(&fixture.group.group_id, &fixture.actor.id).expect("cleanup runtime");
}

#[test]
fn resume_failure_ignores_a_previous_session_archive() {
    let fixture = Fixture::new("resume archive boundary");
    let actor_dir = fixture._temp.path().join("transcripts");
    std::fs::create_dir_all(&actor_dir).expect("transcript dir");
    let history = |name: &str| HistoryConfig {
        path: actor_dir.join(format!("{name}.pty")),
        max_bytes: 1024 * 1024,
        hot_bytes: 64 * 1024,
        persist: true,
    };
    let launch = |command: &str| LaunchSpec {
        group_id: fixture.group.group_id.clone(),
        actor_id: fixture.actor.id.clone(),
        runner: RunnerKind::Pty,
        command: vec!["sh".into(), "-c".into(), command.into()],
        cwd: fixture.cwd.clone(),
        env: BTreeMap::new(),
        cols: 120,
        rows: 40,
    };

    cccc_runtime::start_with_history(
        launch("printf 'ERROR: No saved session found with ID stale\\n'; sleep 5"),
        history("failed"),
    )
    .expect("failed session");
    wait_for_output(
        &fixture.group.group_id,
        &fixture.actor.id,
        "No saved session",
    );
    cccc_runtime::stop(&fixture.group.group_id, &fixture.actor.id).expect("stop failed session");

    cccc_runtime::start_with_history(
        launch("printf 'resumed successfully\\n'; sleep 5"),
        history("resumed"),
    )
    .expect("resumed session");
    wait_for_output(
        &fixture.group.group_id,
        &fixture.actor.id,
        "resumed successfully",
    );

    assert!(
        cccc_runtime::history(&fixture.group.group_id, &fixture.actor.id, None, 64_000)
            .expect("archived history")
            .data
            .contains("No saved session"),
        "the fixture must retain the old marker in the combined archive"
    );
    assert_eq!(
        runtime_session::resume_failure(&fixture.group.group_id, &fixture.actor.id),
        None
    );
    cccc_runtime::stop(&fixture.group.group_id, &fixture.actor.id).expect("stop resumed session");
}

fn wait_for_output(group_id: &str, actor_id: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if cccc_runtime::retained_history(group_id, actor_id)
            .is_ok_and(|history| history.data.contains(expected))
        {
            return;
        }
        assert!(Instant::now() < deadline, "missing output: {expected}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
