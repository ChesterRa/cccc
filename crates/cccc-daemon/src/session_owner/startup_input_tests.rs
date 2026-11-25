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
            None,
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
        crate::ops::runtime_delivery::DeliveryOutcome::Accepted(None),
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

#[test]
fn antigravity_owner_binding_withholds_during_settle_and_survives_preserve_restart() {
    let temp = tempfile::tempdir().expect("isolated home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init home");
    let actor_home = temp.path().join("onboarded-agy-home");
    let onboarding = actor_home.join(".gemini/antigravity-cli/cache/onboarding.json");
    std::fs::create_dir_all(onboarding.parent().expect("cache dir")).expect("cache dir");
    std::fs::write(&onboarding, r#"{"onboardingComplete": true}"#).expect("write onboarding");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store
        .create("Antigravity owner fixture", "")
        .expect("group");
    let mut actor = Actor::new("agy-member");
    actor.runtime = ActorRuntime::Antigravity;
    actor.runner = RunnerKind::Pty;
    group.actors.push(actor.clone());
    store.save(&group).expect("save actor");
    let _lock = cccc_core::fs::try_exclusive_lock(&home.daemon_dir().join("ccccd.lock"))
        .expect("home lock")
        .expect("private home is unlocked");
    controller::initialize(&home).expect("controller");

    let launch_id = "owner-agy-launch-1";
    let state_file =
        cccc_core::antigravity_hook::state_file_path(&home, &group.group_id, &actor.id, launch_id);
    let received = temp.path().join("received");
    let mut env = std::collections::BTreeMap::new();
    env.insert("HOME".into(), actor_home.to_string_lossy().into_owned());
    env.insert(
        "USERPROFILE".into(),
        actor_home.to_string_lossy().into_owned(),
    );
    env.insert(
        cccc_core::antigravity_hook::ENV_STATE_FILE.into(),
        state_file.to_string_lossy().into_owned(),
    );
    env.insert(
        cccc_core::antigravity_hook::ENV_LAUNCH_ID.into(),
        launch_id.into(),
    );
    let actor_ref = ActorRef {
        group_id: group.group_id.clone(),
        actor_id: actor.id.clone(),
        actor_created_at: actor.created_at.clone(),
        generation: None,
    };
    let terminal = controller::start(
        &home,
        actor_ref,
        PreparedLaunch::Pty {
            spec: cccc_runtime::LaunchSpec {
                group_id: group.group_id.clone(),
                actor_id: actor.id.clone(),
                runner: RunnerKind::Pty,
                command: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    format!(
                        "stty raw -echo; printf '\\033[?2004h'; exec cat > {}",
                        received.display()
                    ),
                ],
                cwd: temp.path().into(),
                env,
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
                runtime: ActorRuntime::Antigravity,
                submit: ActorSubmit::Enter,
            },
            preamble: "startup context".into(),
        },
    )
    .expect("start Antigravity terminal")
    .terminal
    .expect("terminal");

    // Owner recorded the launch binding directly; SessionStart makes it idle.
    cccc_core::antigravity_hook::apply_hook_payload(
        &state_file,
        launch_id,
        &group.group_id,
        &actor.id,
        &serde_json::json!({"hookEventName":"SessionStart","conversationId":"c1"}),
    )
    .expect("session start");
    assert!(crate::ops::working_state::ready_for_mail_notice(
        &home,
        &group.group_id,
        &actor.id,
    ));

    let deliver_by = (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    let mut notice = Event::new("system.notify", &group.group_id);
    notice.by = "system".into();
    notice.data = serde_json::json!({
        "kind":"mail_notice","target_actor_id":actor.id,
        "context":{"actor_id":actor.id,"source_event_ids":["mail-1"],"deliver_by":deliver_by}
    })
    .as_object()
    .cloned()
    .expect("notice");
    cccc_core::ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger"),
        &notice,
    )
    .expect("append notice");

    // Flip to working during the owner's 1.5s Antigravity startup settle wait.
    let flip_file = state_file.clone();
    let flip_group = group.group_id.clone();
    let flip_actor = actor.id.clone();
    let flipper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        cccc_core::antigravity_hook::apply_hook_payload(
            &flip_file,
            launch_id,
            &flip_group,
            &flip_actor,
            &serde_json::json!({"hookEventName":"PreInvocation","conversationId":"c1"}),
        )
        .expect("flip working during owner settle");
    });
    let outcome = controller::submit(
        &home,
        &group,
        &actor,
        std::slice::from_ref(&notice),
        "mail waiting",
        &AtomicBool::new(false),
        &AtomicBool::new(false),
        None,
    )
    .expect("submit early notice");
    flipper.join().expect("flipper");
    assert_eq!(outcome["outcome"], "withheld");
    assert!(
        std::fs::read(&received).unwrap_or_default().is_empty(),
        "owner must not write a withheld early notice to the PTY"
    );
    assert!(matches!(
        super::observe_attempt(
            &home,
            &group.group_id,
            &actor.id,
            &actor.created_at,
            &notice.id,
            true,
        )
        .expect("observe withheld"),
        super::RetainedDelivery::Withdrawn
    ));

    // Preserve agents across a maintenance restart and verify hook state survives.
    controller::finish(&home, ShutdownMode::PreserveAgents, "keep".into()).expect("keep");
    controller::initialize(&home).expect("reattach owner");
    let kept = super::runtime::status(&group.group_id, &actor.id).expect("kept status");
    assert_eq!(kept.pid, terminal.pid);
    assert_eq!(kept.started_at, terminal.started_at);
    assert!(!crate::ops::working_state::ready_for_mail_notice(
        &home,
        &group.group_id,
        &actor.id,
    ));

    cccc_core::antigravity_hook::apply_hook_payload(
        &state_file,
        launch_id,
        &group.group_id,
        &actor.id,
        &serde_json::json!({"hookEventName":"Stop","conversationId":"c1","fullyIdle":true}),
    )
    .expect("stop after keep");
    assert!(crate::ops::working_state::ready_for_mail_notice(
        &home,
        &group.group_id,
        &actor.id,
    ));
    let projected =
        crate::ops::working_state::runtime_actor_fields(&home, &actor, &group.group_id, true);
    assert_eq!(projected["effective_working_state"], "idle");
    assert_eq!(projected["effective_working_reason"], "antigravity_hook");

    controller::finish(&home, ShutdownMode::Stop, "stop".into()).expect("stop");
}

#[test]
fn failed_pty_start_releases_owner_registration_for_immediate_retry() {
    let temp = tempfile::tempdir().expect("isolated home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("PTY retry fixture", "").expect("group");
    let mut actor = Actor::new("agy-retry");
    actor.runtime = ActorRuntime::Antigravity;
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
    let make_launch = |cwd: std::path::PathBuf| PreparedLaunch::Pty {
        spec: cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command: vec!["/bin/sh".into(), "-c".into(), "exec sleep 30".into()],
            cwd,
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
            runtime: ActorRuntime::Antigravity,
            submit: ActorSubmit::Enter,
        },
        preamble: String::new(),
    };

    // Start fails with missing cwd; owner must release its reservation without Stop.
    let Err(err) = controller::start(
        &home,
        actor_ref.clone(),
        make_launch(temp.path().join("missing-cwd")),
    ) else {
        panic!("missing cwd must fail start");
    };
    assert!(!err.to_string().is_empty());

    // Immediately retrying the same actor with a valid cwd must succeed directly.
    let started = controller::start(&home, actor_ref, make_launch(temp.path().into()))
        .expect("retry start after failed PTY launch")
        .terminal
        .expect("running terminal");
    assert!(started.running);

    controller::finish(&home, ShutdownMode::Stop, "stop".into()).expect("stop");
}

#[test]
fn antigravity_fresh_home_onboarding_gate_survives_keep_and_releases_after_wizard() {
    let temp = tempfile::tempdir().expect("isolated home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let actor_home = temp.path().join("agy-home");
    std::fs::create_dir_all(&actor_home).expect("actor home");
    let onboarding = actor_home.join(".gemini/antigravity-cli/cache/onboarding.json");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store
        .create("Antigravity startup fixture", "")
        .expect("group");
    let mut actor = Actor::new("agy1");
    actor.runtime = ActorRuntime::Antigravity;
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
    let mut env = std::collections::BTreeMap::new();
    env.insert("HOME".into(), actor_home.to_string_lossy().into_owned());
    env.insert(
        "USERPROFILE".into(),
        actor_home.to_string_lossy().into_owned(),
    );
    let script = r#"
import os, tty, time
from pathlib import Path
tty.setraw(0)
os.write(1, b'\x1b[?2004hWelcome to Antigravity CLI\r\nTerms of Service & Data Use\r\n[Previous] [Done]\r\n')
Path('painted').touch()
choice = os.read(0, 1)
Path('wizard_key').write_bytes(choice)
os.write(1, b'\x1b[2J\x1b[HTrust this workspace?\r\n> 1. Yes, trust\r\n2. No, exit\r\n')
Path('trust_ready').touch()
trust_choice = os.read(0, 1)
Path('trust_key').write_bytes(trust_choice)
os.write(1, '\x1b[2J\x1b[HAntigravity CLI 1.5.16\r\n──────────\r\n>\r\n──────────\r\n? for shortcuts\r\n'.encode())
Path('prompt_ready').touch()
data = b''
pasted = False
while True:
    byte = os.read(0, 1)
    if not byte:
        raise SystemExit(1)
    if byte in (b'\n', b'\r') and not pasted:
        payload = data.replace(b'\x1b[200~', b'').replace(b'\x1b[201~', b'').strip()
        Path('first').write_bytes(payload)
        break
    data += byte
    if data.endswith(b'\x1b[200~'):
        pasted = True
    elif data.endswith(b'\x1b[201~'):
        pasted = False
time.sleep(30)
"#;
    let terminal = controller::start(
        &home,
        actor_ref.clone(),
        PreparedLaunch::Pty {
            spec: cccc_runtime::LaunchSpec {
                group_id: group.group_id.clone(),
                actor_id: actor.id.clone(),
                runner: RunnerKind::Pty,
                command: vec!["python3".into(), "-u".into(), "-c".into(), script.into()],
                cwd: temp.path().into(),
                env: env.clone(),
                cols: 80,
                rows: 24,
            },
            history: cccc_runtime::HistoryConfig {
                path: temp.path().join("agy-terminal.pty"),
                max_bytes: 65536,
                hot_bytes: 8192,
                persist: false,
            },
            launch: cccc_runtime::ActorLaunch {
                runtime: ActorRuntime::Antigravity,
                submit: ActorSubmit::Enter,
            },
            preamble: String::new(),
        },
    )
    .expect("start antigravity terminal")
    .terminal
    .expect("terminal");
    assert!(terminal.startup_input_pending);
    wait_until(|| temp.path().join("painted").exists());

    let first = Event::new("chat.message", &group.group_id);
    cccc_core::ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &first)
        .expect("source event");
    let send = |event: &Event, text: &str| {
        controller::submit(
            &home,
            &group,
            &actor,
            std::slice::from_ref(event),
            text,
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            None,
        )
        .expect("submit")
    };
    assert_eq!(send(&first, "Reply with OK")["outcome"], "not_written");
    assert!(
        !temp.path().join("wizard_key").exists(),
        "no business byte may enter the onboarding wizard"
    );

    // Owner maintenance preservation keeps the onboarding gate intact.
    controller::finish(&home, ShutdownMode::PreserveAgents, "keep".into()).expect("keep");
    controller::initialize(&home).expect("reconnect");
    let retained = super::runtime::status(&group.group_id, &actor.id).expect("retained status");
    assert_eq!(retained.started_at, terminal.started_at);
    assert!(retained.running && retained.startup_input_pending);
    assert_eq!(send(&first, "Reply with OK")["outcome"], "not_written");

    // Cross-shell onboarding write while wizard is still visible keeps input held.
    std::fs::create_dir_all(onboarding.parent().expect("cache dir")).expect("cache dir");
    std::fs::write(&onboarding, r#"{"onboardingComplete": true}"#).expect("write onboarding");
    assert_eq!(send(&first, "Reply with OK")["outcome"], "not_written");
    assert!(!temp.path().join("wizard_key").exists());

    // User advances wizard into workspace trust -> still held even with onboardingComplete=true.
    super::runtime::write(&group.group_id, &actor.id, b"\r").expect("user completes wizard");
    wait_until(|| temp.path().join("trust_ready").exists());
    assert_eq!(send(&first, "Reply with OK")["outcome"], "not_written");
    assert!(
        !temp.path().join("trust_key").exists(),
        "no business byte may answer the workspace trust prompt"
    );

    // User accepts workspace trust -> bordered chat input appears -> delivery succeeds once.
    super::runtime::write(&group.group_id, &actor.id, b"1").expect("user accepts trust");
    wait_until(|| temp.path().join("prompt_ready").exists());
    wait_until(|| send(&first, "Reply with OK")["outcome"] == "accepted");
    wait_until(|| {
        std::fs::read_to_string(temp.path().join("first")).is_ok_and(|text| text == "Reply with OK")
    });
    assert_eq!(
        std::fs::read(temp.path().join("wizard_key")).expect("wizard key"),
        b"\r"
    );
    assert_eq!(
        std::fs::read(temp.path().join("trust_key")).expect("trust key"),
        b"1"
    );
    super::runtime::stop(&group.group_id, &actor.id).expect("stop ready terminal");

    // Early exit before onboarding completes pauses auto-wake until explicit user Start.
    std::fs::remove_file(&onboarding).expect("reset fresh home");
    let mcp_path = actor_home.join(".gemini/config/mcp_config.json");
    std::fs::create_dir_all(mcp_path.parent().expect("mcp dir")).expect("mcp dir");
    std::fs::write(
        &mcp_path,
        r#"{"mcpServers":{"cccc":{"command":"cccc","args":["mcp"],"disabled":false}}}"#,
    )
    .expect("mcp config");
    let settings_path = actor_home.join(".gemini/antigravity-cli/settings.json");
    std::fs::create_dir_all(settings_path.parent().expect("settings dir")).expect("settings dir");
    std::fs::write(&settings_path, r#"{"showFeedbackSurvey":false}"#).expect("settings");
    let scope_url = temp.path().to_string_lossy().into_owned();
    let scope_key = "scope_agy".to_owned();
    group.active_scope_key = scope_key.clone();
    group.scopes.push(cccc_core::Scope {
        scope_key,
        url: scope_url,
        label: "workspace".into(),
        git_remote: String::new(),
    });
    group.actors[0].command = vec![
        "python3".into(),
        "-u".into(),
        "-c".into(),
        "import time; time.sleep(30)".into(),
    ];
    group.actors[0].env = env.clone();
    store.save(&group).expect("save actor for user start");
    actor = group.actors[0].clone();

    let closed = controller::start(
        &home,
        actor_ref,
        PreparedLaunch::Pty {
            spec: cccc_runtime::LaunchSpec {
                group_id: group.group_id.clone(),
                actor_id: actor.id.clone(),
                runner: RunnerKind::Pty,
                command: vec![
                    "python3".into(),
                    "-u".into(),
                    "-c".into(),
                    "raise SystemExit(0)".into(),
                ],
                cwd: temp.path().into(),
                env,
                cols: 80,
                rows: 24,
            },
            history: cccc_runtime::HistoryConfig {
                path: temp.path().join("agy-closed.pty"),
                max_bytes: 65536,
                hot_bytes: 8192,
                persist: false,
            },
            launch: cccc_runtime::ActorLaunch {
                runtime: ActorRuntime::Antigravity,
                submit: ActorSubmit::Enter,
            },
            preamble: String::new(),
        },
    )
    .expect("start exiting fresh antigravity")
    .terminal
    .expect("closed terminal");
    wait_until(|| {
        super::runtime::status(&group.group_id, &actor.id)
            .is_ok_and(|status| !status.running && status.startup_input_pending)
    });
    let exited = crate::ops::actor_runtime::reap_exited().expect("reap exited");
    assert!(
        exited
            .iter()
            .any(|status| status.group_id == group.group_id
                && status.started_at == closed.started_at)
    );
    crate::ops::actor_runtime::reconcile_exited(&home, exited).expect("reconcile exit");

    let list_paused = crate::handle_request(
        &home,
        &cccc_contracts::DaemonRequest {
            v: 1,
            op: "actor_list".into(),
            args: serde_json::json!({"group_id": group.group_id})
                .as_object()
                .expect("args")
                .clone(),
        },
    );
    assert!(list_paused.ok, "{:?}", list_paused.error);
    assert_eq!(
        list_paused.result["actors"][0]["runtime_confirmation_paused"],
        serde_json::json!(true),
        "early exit before onboarding must report runtime_confirmation_paused"
    );
    let auto_wake = crate::ops::actor_runtime::apply(&home, &group, &actor.id, "actor.start")
        .expect_err("automatic wake must remain paused");
    assert_eq!(auto_wake.code, "runtime_confirmation_paused");

    // Explicit user actor_start clears the pause and launches a fresh guarded terminal.
    let user_start = crate::handle_request(
        &home,
        &cccc_contracts::DaemonRequest {
            v: 1,
            op: "actor_start".into(),
            args: serde_json::json!({
                "group_id": group.group_id,
                "actor_id": actor.id,
                "by": "user",
            })
            .as_object()
            .expect("args")
            .clone(),
        },
    );
    assert!(user_start.ok, "{:?}", user_start.error);
    let restarted = super::runtime::status(&group.group_id, &actor.id).expect("restarted status");
    assert!(restarted.running);
    assert_ne!(restarted.started_at, closed.started_at);
    assert!(restarted.startup_input_pending);
    let list_running = crate::handle_request(
        &home,
        &cccc_contracts::DaemonRequest {
            v: 1,
            op: "actor_list".into(),
            args: serde_json::json!({"group_id": group.group_id})
                .as_object()
                .expect("args")
                .clone(),
        },
    );
    assert!(list_running.ok, "{:?}", list_running.error);
    assert_eq!(
        list_running.result["actors"][0]["runtime_confirmation_paused"],
        serde_json::json!(false)
    );
    controller::finish(&home, ShutdownMode::Stop, "stop".into()).expect("stop");
}
