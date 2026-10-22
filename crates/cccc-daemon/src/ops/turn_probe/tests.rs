use super::*;
use process::Processes;
use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hermes-turn-probe")
        .join(name)
}

struct Capture {
    metadata: Value,
    processes: BTreeMap<u32, Process>,
}

impl Capture {
    fn load() -> Self {
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(fixture("capture.json")).expect("capture"))
                .expect("metadata");
        let processes = metadata["processes"]
            .as_object()
            .expect("processes")
            .values()
            .map(|p| {
                let process = Process {
                    pid: p["pid"].as_u64().expect("pid") as u32,
                    parent: p["ppid"].as_u64().expect("parent") as u32,
                    start: p["start_ticks"].as_u64().expect("start"),
                    namespace: p["pid_namespace"].as_u64().expect("namespace"),
                    born_before: p["born_before"].as_f64().expect("birth"),
                };
                (process.pid, process)
            })
            .collect();
        Self {
            metadata,
            processes,
        }
    }

    fn root(&self) -> Process {
        self.read(self.metadata["target"]["root_pid"].as_u64().expect("root") as u32)
            .expect("root process")
    }

    fn now(&self) -> f64 {
        self.metadata["captured_at_unix"]
            .as_f64()
            .expect("capture time")
    }
}

impl Processes for Capture {
    fn read(&self, pid: u32) -> Result<Process, &'static str> {
        self.processes
            .get(&pid)
            .cloned()
            .ok_or("process_unavailable")
    }
}

fn copy_database() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("state.db");
    std::fs::copy(fixture("during.sqlite3"), &path).expect("copy real metadata");
    (temp, path)
}

#[test]
fn real_lease_is_working_then_provider_expiry_is_idle_only_after_binding() {
    let capture = Capture::load();
    let root = capture.root();
    let path = fixture("during.sqlite3");
    let before = std::fs::read(&path).expect("before");
    let mut probe = hermes::Hermes::default();
    assert!(
        matches!(
            probe.observe(&path, &capture, &root, capture.now()),
            TurnObservation::Working { .. }
        ),
        "a real owned unexpired Hermes lease must be working"
    );
    let last_expiry: f64 = Connection::open(&path)
        .expect("fixture")
        .query_row(
            "SELECT MAX(expires_at) FROM session_turn_leases",
            [],
            |row| row.get(0),
        )
        .expect("expiry");
    // Replay at the provider's own expiry; no invented lease or session rows.
    assert_eq!(
        probe.observe(&path, &capture, &root, last_expiry + 1.0),
        TurnObservation::Idle
    );
    assert!(
        matches!(
            hermes::Hermes::default().observe(&path, &capture, &root, last_expiry + 1.0),
            TurnObservation::Unknown { .. }
        ),
        "no lease alone cannot bind an idle session"
    );
    assert_eq!(
        std::fs::read(&path).expect("after"),
        before,
        "probe must not write provider bytes"
    );
}

#[test]
fn foreign_lease_cannot_establish_an_actor_binding() {
    let capture = Capture::load();
    let root = capture.root();
    let (_temp, path) = copy_database();
    // Negative control on a copy of the capture: remove this Actor's real lease.
    Connection::open(&path)
        .expect("copy")
        .execute(
            "DELETE FROM session_turn_leases WHERE conversation_id=?1",
            [capture.metadata["target"]["conversation_id"]
                .as_str()
                .expect("session")],
        )
        .expect("remove owned lease");
    assert!(matches!(
        hermes::Hermes::default().observe(&path, &capture, &root, capture.now()),
        TurnObservation::Unknown { .. }
    ));
}

#[test]
fn unavailable_schema_lock_and_missing_session_fail_closed() {
    let capture = Capture::load();
    for alteration in [
        "DROP TABLE session_turn_leases",
        "UPDATE schema_version SET version=32",
    ] {
        let (_temp, path) = copy_database();
        Connection::open(&path)
            .expect("copy")
            .execute_batch(alteration)
            .expect("negative control");
        assert!(matches!(
            hermes::Hermes::default().observe(&path, &capture, &capture.root(), capture.now()),
            TurnObservation::Unknown { .. }
        ));
    }
    let (_temp, path) = copy_database();
    let writer = Connection::open(&path).expect("copy");
    writer
        .execute_batch("PRAGMA journal_mode=DELETE; BEGIN EXCLUSIVE")
        .expect("lock");
    let start = std::time::Instant::now();
    assert!(matches!(
        hermes::Hermes::default().observe(&path, &capture, &capture.root(), capture.now()),
        TurnObservation::Unknown { .. }
    ));
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    writer.execute_batch("ROLLBACK").expect("unlock");
    let mut probe = hermes::Hermes::default();
    assert!(matches!(
        probe.observe(&path, &capture, &capture.root(), capture.now()),
        TurnObservation::Working { .. }
    ));
    writer
        .execute_batch("DELETE FROM sessions")
        .expect("missing sessions control");
    let later: f64 = writer
        .query_row(
            "SELECT MAX(expires_at)+1 FROM session_turn_leases",
            [],
            |row| row.get(0),
        )
        .expect("expiry");
    assert!(matches!(
        probe.observe(&path, &capture, &capture.root(), later),
        TurnObservation::Unknown { .. }
    ));
}

#[test]
fn pid_reuse_namespace_and_generation_changes_never_reuse_authority() {
    let mut capture = Capture::load();
    let old_root = capture.root();
    let path = fixture("during.sqlite3");
    let mut probe = hermes::Hermes::default();
    assert!(matches!(
        probe.observe(&path, &capture, &old_root, capture.now()),
        TurnObservation::Working { .. }
    ));
    capture
        .processes
        .get_mut(&old_root.pid)
        .expect("root")
        .start += 1;
    assert!(matches!(
        probe.observe(&path, &capture, &old_root, capture.now()),
        TurnObservation::Unknown { .. }
    ));
    let mut reused = capture.root();
    reused.born_before = capture.now() + 1.0;
    capture.processes.insert(reused.pid, reused.clone());
    assert!(matches!(
        hermes::Hermes::default().observe(&path, &capture, &reused, capture.now()),
        TurnObservation::Unknown { .. }
    ));
    reused.namespace += 1;
    capture.processes.insert(reused.pid, reused.clone());
    assert!(matches!(
        hermes::Hermes::default().observe(&path, &capture, &reused, capture.now()),
        TurnObservation::Unknown { .. }
    ));
}

#[test]
fn defaults_are_absent_and_provider_conversation_is_not_a_task_id() {
    let actor = Actor::new("peer");
    let mut value = serde_json::to_value(&actor).expect("serialize");
    assert!(value.get("turn_probe").is_none());
    value["turn_probe"] = serde_json::json!("auto");
    let enabled: Actor = serde_json::from_value(value).expect("opt in");
    assert_eq!(enabled.turn_probe, TurnProbe::Auto);
    let (_, reason, _, task) = fields(TurnObservation::Working { since: None });
    assert_eq!(reason, "turn_probe:hermes_turn_lease");
    assert_eq!(task, None);
}

#[test]
fn captured_hermes_lease_populates_existing_working_fields() {
    let capture = Capture::load();
    let observation = hermes::Hermes::default().observe(
        &fixture("during.sqlite3"),
        &capture,
        &capture.root(),
        capture.now(),
    );
    assert!(matches!(observation, TurnObservation::Working { .. }));
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
    let mut actor = Actor::new("peer");
    actor.runtime = ActorRuntime::Hermes;
    actor.turn_probe = TurnProbe::Auto;
    let state = super::super::working_state::fields_with_probe(
        &home,
        &actor,
        "fixture",
        true,
        "pty",
        || Some(observation),
    );
    assert_eq!(
        state["effective_working_state"], "working",
        "a captured owned Hermes turn must replace the native-terminal unknown fallback"
    );
    assert_eq!(
        state["effective_working_reason"],
        "turn_probe:hermes_turn_lease"
    );
    assert_eq!(state["effective_active_task_id"], serde_json::Value::Null);
    assert!(!state.contains_key("turn_probe"));
}

#[test]
fn stopped_and_headless_states_never_consult_the_probe() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
    let mut actor = Actor::new("peer");
    actor.runtime = ActorRuntime::Custom;
    for (running, runner, expected) in [(false, "pty", "stopped"), (true, "headless", "idle")] {
        let state = super::super::working_state::fields_with_probe(
            &home,
            &actor,
            "fixture",
            running,
            runner,
            || panic!("probe must not run on this surface"),
        );
        assert_eq!(state["effective_working_state"], expected);
    }
}
