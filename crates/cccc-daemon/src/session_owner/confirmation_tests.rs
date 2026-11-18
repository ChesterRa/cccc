//! Hermes's first setup run by a separate session owner: the daemon must act
//! on the owner's confirmation state, not its own tables.
use super::{ShutdownMode, controller, protocol::*};
use crate::ops::{actor_runtime, local_headless};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, RuntimeMode};
use cccc_core::{GroupDoc, GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Fixture {
    temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    actor: Actor,
}

impl Fixture {
    /// Marker files in the workspace switch on behaviours of the ACP fixture.
    fn new(markers: &[&str]) -> Self {
        let temp = tempfile::tempdir().expect("isolated fixture");
        for marker in markers {
            std::fs::write(temp.path().join(marker), "").expect("fixture marker");
        }
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
        home.initialize().expect("initialize home");
        let store = GroupStore::new(home.clone()).expect("store");
        let created = store.create("Hermes setup", "").expect("group");
        let mut group = cccc_core::group_scope::attach(
            &store,
            &created.group_id,
            cccc_core::scope::detect(temp.path()).expect("fixture scope"),
        )
        .expect("attach scope");
        let executable = temp.path().join("hermes");
        std::fs::write(
            &executable,
            include_str!("../ops/codex_voice_analyst/native_acp/v3_fixture.py"),
        )
        .expect("write ACP fixture");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("fixture permissions");
        let mut actor = Actor::new("worker");
        actor.runtime = ActorRuntime::Hermes;
        actor.runtime_mode = RuntimeMode::Default;
        actor.command = vec![executable.to_string_lossy().into_owned()];
        actor.env = BTreeMap::from([
            (
                "HOME".into(),
                temp.path().join("provider").to_string_lossy().into_owned(),
            ),
            ("ACP_FIXTURE_CLI".into(), "/bin/true".into()),
        ]);
        group.actors.push(actor.clone());
        group.running = true;
        store.save(&group).expect("save actor");
        Self {
            temp,
            home,
            group,
            actor,
        }
    }

    fn setup_runs(&self) -> usize {
        std::fs::read_to_string(self.temp.path().join("v3_setup_runs.jsonl"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// Each ACP process the member's starts launched begins with `initialize`.
    fn launches(&self) -> usize {
        std::fs::read_to_string(self.temp.path().join("v3_requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|request| request["method"] == "initialize")
            .count()
    }

    fn setup_pid(&self) -> Option<u32> {
        actor_runtime::status(&self.group.group_id, "worker")
            .filter(|status| status.running)
            .and_then(|status| status.pid)
    }

    fn wait(&self, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < deadline, "fixture event deadline");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn request(&self, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
        let mut args = args.as_object().expect("arguments").clone();
        args.insert("group_id".into(), json!(self.group.group_id));
        args.insert("by".into(), json!("user"));
        crate::handle_request(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            },
        )
    }
}

/// A setup left without a provider pauses inside the owner. An automatic start
/// or restore is refused there before it retires the member, and Stop retires
/// the pause with the member, so the same owner opens the setup again on the
/// next start.
#[test]
fn a_left_setup_keeps_its_pause_in_the_owner_until_the_member_is_retired() {
    let fixture = Fixture::new(&["v3_setup_needed", "v3_setup_quit"]);
    let group = fixture.group.group_id.clone();
    let _lock = cccc_core::fs::try_exclusive_lock(&fixture.home.daemon_dir().join("ccccd.lock"))
        .expect("home lock")
        .expect("private home is unlocked");
    controller::initialize(&fixture.home).expect("controller");
    crate::session_owner::launch_managed(&fixture.home, &fixture.group, &fixture.actor)
        .expect("start in the owner");
    fixture.wait(|| fixture.setup_runs() == 1);
    let identity = controller::snapshot(&group, "worker")
        .expect("snapshot")
        .expect("owned member")
        .actor;
    let owner = controller::client_for(&identity).expect("owner");
    std::fs::write(fixture.temp.path().join("v3_setup_finish"), "").expect("leave setup");
    fixture.wait(|| local_headless::confirmation_paused(&group, "worker").expect("pause"));
    // The watcher pauses before it retires; a start in between only waits.
    fixture
        .wait(|| !local_headless::confirmation_pending_active(&group, "worker").expect("recovery"));

    let automatic = actor_runtime::apply(&fixture.home, &fixture.group, "worker", "actor.start")
        .expect_err("automatic starts wait for the user");
    assert_eq!(
        automatic.code,
        actor_runtime::RUNTIME_CONFIRMATION_PAUSED,
        "{}",
        automatic.message
    );
    assert!(local_headless::confirmation_paused(&group, "worker").expect("still shown"));
    // A daemon restart's restore holds it the same way, before any teardown.
    crate::ops::runtime_restore::restore_running(&fixture.home).expect("restore");
    assert!(local_headless::confirmation_paused(&group, "worker").expect("after restore"));
    assert_eq!(fixture.setup_runs(), 1);

    actor_runtime::apply(&fixture.home, &fixture.group, "worker", "actor.stop")
        .expect("the user's stop");
    assert!(
        controller::snapshot(&group, "worker")
            .expect("owner")
            .is_none()
    );
    // Test builds carry no source identity, so the controller would give a new
    // start a new owner. Ask the same one: it must not keep the retired pause.
    let start = owner
        .mutate(Mutation::Start {
            actor: ActorRef {
                generation: None,
                ..identity
            },
            launch: Box::new(PreparedLaunch::Managed {
                group: fixture.group.clone(),
                actor: fixture.actor.clone(),
                host_environment: std::env::vars().collect(),
            }),
        })
        .expect("admit the user's start");
    owner
        .wait(&start, Duration::from_secs(30))
        .expect("the setup opens again");
    owner.acknowledge(&start).expect("acknowledge the start");
    fixture.wait(|| fixture.setup_runs() == 2);
    controller::finish(&fixture.home, ShutdownMode::Stop, "stop".into()).expect("stop");
}

/// While the setup runs, the user's Start, a save of only Enabled, a Group start
/// and a daemon restart's restore keep the same setup process and its recovery,
/// so the user's typing is not lost; finishing the setup then connects the member.
#[test]
fn starting_again_keeps_an_open_setup_in_the_owner() {
    let fixture = Fixture::new(&["v3_setup_needed"]);
    let group = fixture.group.group_id.clone();
    let _lock = cccc_core::fs::try_exclusive_lock(&fixture.home.daemon_dir().join("ccccd.lock"))
        .expect("home lock")
        .expect("private home is unlocked");
    controller::initialize(&fixture.home).expect("controller");
    crate::session_owner::launch_managed(&fixture.home, &fixture.group, &fixture.actor)
        .expect("start in the owner");
    fixture.wait(|| fixture.setup_runs() == 1);
    let setup = fixture.setup_pid().expect("setup terminal");
    for (op, args) in [
        ("actor_start", json!({"actor_id":"worker"})),
        (
            "actor_update",
            json!({"actor_id":"worker","patch":{"enabled":true}}),
        ),
        ("group_start", json!({})),
    ] {
        let response = fixture.request(op, args);
        assert!(response.ok, "{op}: {:?}", response.error);
        assert_eq!(fixture.setup_pid(), Some(setup), "{op}");
        assert!(
            local_headless::confirmation_pending_active(&group, "worker").expect("setup"),
            "{op}"
        );
    }
    // A daemon restart's restore finds the setup still open in its owner.
    crate::ops::runtime_restore::restore_running(&fixture.home).expect("restore");
    assert_eq!(fixture.setup_pid(), Some(setup), "restore");
    assert!(local_headless::confirmation_pending_active(&group, "worker").expect("setup"));
    assert_eq!(fixture.setup_runs(), 1);
    std::fs::write(fixture.temp.path().join("v3_setup_finish"), "").expect("finish setup");
    fixture.wait(|| local_headless::running(&group, "worker"));
    assert_eq!(fixture.setup_runs(), 1);
    controller::finish(&fixture.home, ShutdownMode::Stop, "stop".into()).expect("stop");
}

/// The setup connects the member after a start found it not running but before
/// that start looked at the setup, as a repeated Start, a delivery or a restore
/// can: the start keeps the session the setup connected instead of replacing it.
#[test]
fn a_start_that_meets_the_setup_connecting_keeps_its_session() {
    let fixture = Fixture::new(&["v3_setup_needed"]);
    let group = fixture.group.group_id.clone();
    let _lock = cccc_core::fs::try_exclusive_lock(&fixture.home.daemon_dir().join("ccccd.lock"))
        .expect("home lock")
        .expect("private home is unlocked");
    controller::initialize(&fixture.home).expect("controller");
    crate::session_owner::launch_managed(&fixture.home, &fixture.group, &fixture.actor)
        .expect("start in the owner");
    fixture.wait(|| fixture.setup_runs() == 1);
    let connected = Arc::new(Mutex::new(None));
    let (seen, root, member_group) = (
        Arc::clone(&connected),
        fixture.temp.path().to_owned(),
        group.clone(),
    );
    actor_runtime::start_interleaving::after_running_check(&group, "worker", move || {
        std::fs::write(root.join("v3_setup_finish"), "").expect("finish setup");
        let deadline = Instant::now() + Duration::from_secs(10);
        while local_headless::confirmation_pending_active(&member_group, "worker")
            .expect("recovery")
            || !local_headless::running(&member_group, "worker")
        {
            assert!(Instant::now() < deadline, "the setup connects the member");
            std::thread::sleep(Duration::from_millis(20));
        }
        *seen.lock().expect("generation") =
            local_headless::registered_generation(&member_group, "worker");
    });

    actor_runtime::apply(&fixture.home, &fixture.group, "worker", "actor.start")
        .expect("the start");
    let connected = connected
        .lock()
        .expect("generation")
        .clone()
        .expect("the setup connected the member during the start");
    assert_eq!(
        local_headless::registered_generation(&group, "worker"),
        Some(connected)
    );
    // The first launch found no provider and opened the setup; the setup's own
    // connection is the second. A replacement would be a third.
    assert_eq!(fixture.launches(), 2);
    controller::finish(&fixture.home, ShutdownMode::Stop, "stop".into()).expect("stop");
}
