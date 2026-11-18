use super::{ShutdownMode, controller, protocol::*};
use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, Event, RunnerKind};
use cccc_core::{GroupStore, HomeLayout};
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn wait_until(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(
            Instant::now() < deadline,
            "owner startup fixture did not advance"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_unsubmitted_startup_input_retries_once_and_stays_released_after_keep() {
    let temp = tempfile::tempdir().expect("isolated home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Startup fixture", "").expect("group");
    let mut actor = Actor::new("member");
    actor.runtime = ActorRuntime::Copilot;
    actor.runner = RunnerKind::Pty;
    group.actors.push(actor.clone());
    store.save(&group).expect("save actor");
    let _lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
        .expect("home lock")
        .expect("private home is unlocked");
    controller::initialize(&home).expect("controller");
    let actor_ref = ActorRef {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        actor_created_at: actor.created_at.clone(),
        generation: None,
    };
    let start = |script: &str| {
        controller::start(
            &home,
            actor_ref.clone(),
            PreparedLaunch::Pty {
                spec: cccc_runtime::LaunchSpec {
                    group_id: group.group_id.clone(),
                    actor_id: actor.id.clone(),
                    runner: RunnerKind::Pty,
                    command: vec!["python3".into(), "-u".into(), "-c".into(), script.into()],
                    cwd: temp.path().into(),
                    env: Default::default(),
                    cols: 80,
                    rows: 24,
                },
                history: cccc_runtime::HistoryConfig {
                    path: temp.path().join("terminal.pty"),
                    max_bytes: 65536,
                    hot_bytes: 8192,
                    persist: false,
                },
                launch: cccc_runtime::ActorLaunch {
                    runtime: ActorRuntime::Copilot,
                    submit: ActorSubmit::Newline,
                },
                preamble: String::new(),
            },
        )
        .expect("start held terminal")
        .terminal
        .expect("terminal")
    };
    let terminal = start(
        r#"
import os, tty
from pathlib import Path
tty.setraw(0)
os.write(1, b'\x1b[?2004hConfirm folder trust\r\n')
Path('painted').touch()
choice = os.read(0, 1)
Path('choice').write_bytes(choice)
os.write(1, '\x1b[2J\x1b[H──────────\r\n❯\r\n──────────\r\n'.encode())
def read_submission():
    data = b''
    pasted = False
    while True:
        byte = os.read(0, 1)
        if not byte:
            raise SystemExit(1)
        if byte in (b'\n', b'\r') and not pasted:
            return data.replace(b'\x1b[200~', b'').replace(b'\x1b[201~', b'').strip()
        data += byte
        if data.endswith(b'\x1b[200~'):
            pasted = True
        elif data.endswith(b'\x1b[201~'):
            pasted = False
Path('first').write_bytes(read_submission())
os.write(1, b'\x1b[2J\x1b[HWorking')
Path('second').write_bytes(read_submission())
import time
time.sleep(30)
"#,
    );
    wait_until(|| temp.path().join("painted").exists());
    let identity = controller::snapshot(&group.group_id, &actor.id)
        .expect("snapshot")
        .expect("owned actor")
        .actor;
    let client = controller::client_for(&identity).expect("client");
    let first = Event::new("chat.message", &group.group_id);
    cccc_core::ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &first)
        .expect("source event");
    let source = OperationSource {
        actor: identity.clone(),
        event_ids: vec![first.id.clone()],
        input_sha256: format!("{:x}", Sha256::digest(b"first")),
    };
    // Exercise the final writer check even when an earlier observation looked
    // ready: this admitted operation must prove zero bytes before it is reclaimed.
    let operation = client
        .mutate(Mutation::Submit {
            source: source.clone(),
            group: Box::new(group.clone()),
            actor: Box::new(actor.clone()),
            events: vec![first.clone()],
            text: "first".into(),
        })
        .expect("admit guarded attempt");
    let held = client
        .wait(&operation, Duration::from_secs(10))
        .expect("zero-input result");
    assert_eq!(held["outcome"], "not_written");
    let send = |event: &Event, text: &str| {
        controller::submit(
            &home,
            &group,
            &actor,
            std::slice::from_ref(event),
            text,
            &AtomicBool::new(false),
            &AtomicBool::new(false),
        )
        .expect("submit")
    };
    assert_eq!(send(&first, "first")["outcome"], "not_written");
    assert!(client.find(&source).expect("reclaimed source").is_empty());
    assert!(
        !temp.path().join("choice").exists(),
        "business input did not answer trust"
    );
    let keep = || {
        controller::finish(&home, ShutdownMode::PreserveAgents, "keep".into())
            .expect("keep held terminal");
        controller::initialize(&home).expect("replacement daemon controller");
        super::runtime::status(&group.group_id, &actor.id).expect("held terminal")
    };
    let waiting = keep();
    assert_eq!(waiting.started_at, terminal.started_at);
    assert!(waiting.running && waiting.startup_input_pending);
    assert_eq!(send(&first, "first")["outcome"], "not_written");
    assert!(!temp.path().join("choice").exists());
    super::runtime::write(&group.group_id, &actor.id, b"a")
        .expect("user answers in native terminal");
    wait_until(|| send(&first, "first")["outcome"] == "accepted");
    crate::ops::runtime_delivery::append_state(
        &home,
        &group.group_id,
        &actor.id,
        &actor.created_at,
        &first.id,
        "pty",
        crate::ops::runtime_delivery::DeliveryOutcome::Accepted,
    )
    .expect("durable accepted delivery");
    wait_until(|| {
        std::fs::read_to_string(temp.path().join("first")).is_ok_and(|text| text == "first")
    });
    assert_eq!(
        std::fs::read_to_string(temp.path().join("choice")).expect("choice"),
        "a"
    );
    let kept = keep();
    assert_eq!(kept.pid, terminal.pid);
    assert_eq!(kept.started_at, terminal.started_at);
    assert!(
        !kept.startup_input_pending,
        "Keep must preserve the passed first-input gate"
    );
    let second = Event::new("chat.message", &group.group_id);
    assert_eq!(send(&second, "second")["outcome"], "accepted");
    wait_until(|| {
        std::fs::read_to_string(temp.path().join("second")).is_ok_and(|text| text == "second")
    });
    super::runtime::stop(&group.group_id, &actor.id).expect("stop ready terminal");
    let closed = start("raise SystemExit(0)");
    wait_until(|| {
        super::runtime::status(&group.group_id, &actor.id)
            .is_ok_and(|status| !status.running && status.startup_input_pending)
    });
    let observe_exit =
        || {
            use crate::ops::actor_runtime::{reap_exited, reconcile_exited};
            let exited = reap_exited().expect("observe closed held terminal");
            assert!(exited.iter().any(|status| status.group_id == group.group_id
                && status.started_at == closed.started_at));
            reconcile_exited(&home, exited).expect("record exit");
        };
    observe_exit();
    let closed_after_keep = keep();
    assert_eq!(closed_after_keep.started_at, closed.started_at);
    assert!(!closed_after_keep.running && closed_after_keep.startup_input_pending);
    observe_exit();
    assert_eq!(
        cccc_core::ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("read exit events")
            .iter()
            .filter(|event| event.kind == "actor.stop")
            .count(),
        1
    );
    controller::finish(&home, ShutdownMode::Stop, "stop".into()).expect("stop");
}
