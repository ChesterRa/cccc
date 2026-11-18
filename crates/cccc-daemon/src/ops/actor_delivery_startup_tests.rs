//! Offline PTY fixtures for ready input and the early paste-mode startup race.
//! They do not establish provider receipt or completion of login/trust dialogs.
use super::*;
use cccc_contracts::{DaemonRequest, Event, RunnerKind};
use cccc_core::{GroupStore, HomeLayout};
use std::collections::BTreeMap;

#[test]
fn actor_start_preserves_a_healthy_acp_owner_despite_a_failed_native_receipt() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("isolated ACP fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("healthy ACP Start", "").expect("group");
    let mut group = cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::scope::detect(temp.path()).expect("fixture scope"),
    )
    .expect("attach scope");
    let executable = temp.path().join("kiro-cli");
    std::fs::write(
        &executable,
        include_str!("codex_voice_analyst/native_acp/v3_fixture.py"),
    )
    .expect("write official ACP shape fixture");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fixture permissions");
    let mut actor = Actor::new("peer");
    actor.runtime = ActorRuntime::Kiro;
    actor.runtime_mode = cccc_contracts::RuntimeMode::Acp;
    actor.command = vec![executable.to_string_lossy().into_owned()];
    actor.env = BTreeMap::from([
        (
            "HOME".into(),
            temp.path().join("provider").to_string_lossy().into_owned(),
        ),
        (
            "CODEX_HOME".into(),
            temp.path().join("codex").to_string_lossy().into_owned(),
        ),
        ("ACP_FIXTURE_CLI".into(), "/bin/true".into()),
    ]);
    group.actors.push(actor.clone());
    store.save(&group).expect("save ACP actor");
    let start = || {
        crate::handle_request(
            &home,
            &DaemonRequest {
                v: 1,
                op: "actor_start".into(),
                args: serde_json::json!({"group_id":group.group_id,"actor_id":"peer","by":"user"})
                    .as_object()
                    .expect("start arguments")
                    .clone(),
            },
        )
    };
    let initial = start();
    assert!(initial.ok, "{:?}", initial.error);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let generation = crate::ops::local_headless::structured_state(&group.group_id, "peer")
            .expect("managed owner")["generation"]
            .clone();
        let (receipt, retained) = stale_failed_codex_receipt(&home, &group.group_id, temp.path());
        for saved_runtime in [ActorRuntime::Kiro, ActorRuntime::Claude] {
            store
                .mutate(&group.group_id, |doc| {
                    doc.actors[0].runtime = saved_runtime;
                    doc.actors[0].runtime_mode = if saved_runtime == ActorRuntime::Kiro {
                        cccc_contracts::RuntimeMode::Acp
                    } else {
                        cccc_contracts::RuntimeMode::Default
                    };
                    Ok(())
                })
                .expect("save next-launch runtime");
            let repeated = start();
            assert!(repeated.ok, "{:?}", repeated.error);
            assert_eq!(
                crate::ops::local_headless::structured_state(&group.group_id, "peer")
                    .expect("same managed owner")["generation"],
                generation,
                "Start must not replace a healthy ACP owner"
            );
            assert_eq!(std::fs::read(&receipt).expect("retained receipt"), retained);
        }
        let requests =
            std::fs::read_to_string(temp.path().join("v3_requests.jsonl")).expect("ACP requests");
        assert_eq!(
            requests
                .lines()
                .filter(|line| serde_json::from_str::<serde_json::Value>(line)
                    .expect("RPC frame")["method"]
                    == "session/new")
                .count(),
            1,
            "idempotent Start must not create another provider conversation"
        );
    }));
    crate::ops::local_headless::stop(&group.group_id, "peer").expect("clean up managed fixture");
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn actor_start_preserves_a_healthy_native_owner_after_saved_runtime_changes() {
    for runtime in [ActorRuntime::Kiro, ActorRuntime::Claude] {
        let temp = tempfile::tempdir().expect("isolated native fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("healthy native Start", "").expect("group");
        let mut group = cccc_core::group_scope::attach(
            &store,
            &group.group_id,
            cccc_core::scope::detect(temp.path()).expect("fixture scope"),
        )
        .expect("attach native fixture scope");
        let mut actor = Actor::new("peer");
        actor.runtime = runtime;
        actor.runtime_mode = if runtime == ActorRuntime::Kiro {
            cccc_contracts::RuntimeMode::Acp
        } else {
            cccc_contracts::RuntimeMode::Default
        };
        actor.command = vec!["/bin/false".into()];
        actor.env.insert(
            "CODEX_HOME".into(),
            temp.path().join("codex").to_string_lossy().into_owned(),
        );
        group.actors.push(actor);
        store.save(&group).expect("save next-launch settings");
        let terminal = cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: "peer".into(),
            runner: RunnerKind::Pty,
            command: vec!["/bin/sh".into(), "-c".into(), "exec sleep 60".into()],
            cwd: temp.path().to_path_buf(),
            env: BTreeMap::new(),
            cols: 120,
            rows: 40,
        })
        .expect("start native fixture");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (receipt, retained) =
                stale_failed_codex_receipt(&home, &group.group_id, temp.path());
            let response = crate::handle_request(
                &home,
                &DaemonRequest {
                    v: 1,
                    op: "actor_start".into(),
                    args:
                        serde_json::json!({"group_id":group.group_id,"actor_id":"peer","by":"user"})
                            .as_object()
                            .expect("start arguments")
                            .clone(),
                },
            );
            assert_eq!(
                cccc_runtime::status(&group.group_id, "peer")
                    .ok()
                    .filter(|status| status.running)
                    .and_then(|status| status.pid),
                terminal.pid,
                "Start must not apply saved settings to a healthy native owner"
            );
            assert!(response.ok, "{:?}", response.error);
            assert_eq!(
                std::fs::read(&receipt).expect("old receipt retained"),
                retained
            );
        }));
        if cccc_runtime::status(&group.group_id, "peer").is_ok_and(|status| status.running) {
            cccc_runtime::stop(&group.group_id, "peer").expect("clean up native fixture");
        }
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

fn stale_failed_codex_receipt(
    home: &HomeLayout,
    group: &str,
    cwd: &std::path::Path,
) -> (std::path::PathBuf, Vec<u8>) {
    crate::ops::runtime_session::record_codex_app_thread(
        home,
        group,
        "peer",
        cwd,
        &["codex".into()],
        &BTreeMap::new(),
        crate::ops::runtime_session::CodexAppThread {
            id: "retained-native-conversation",
            resumed: true,
        },
    )
    .expect("record prior native conversation");
    let path = home
        .groups_dir()
        .join(group)
        .join("state/runtime_sessions/peer.json");
    let mut value: serde_json::Value = cccc_core::fs::read_json(&path).expect("native receipt");
    value["status"] = "resume_failed".into();
    value["resume_eligible"] = false.into();
    cccc_core::fs::write_json(&path, &value).expect("mark retained resume failure");
    let bytes = std::fs::read(&path).expect("retained receipt bytes");
    (path, bytes)
}

#[test]
fn antigravity_delivers_automatically_without_terminal_labels_after_restart() {
    exercise_startup("\x1b[2J\x1b[HAn entirely different UI\r\n>", false, None);
}

#[test]
fn antigravity_first_payload_survives_early_input_mode_during_initialization() {
    exercise_startup("Initializing a native terminal", true, None);
}

#[test]
fn antigravity_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Antigravity);
}

#[test]
fn copilot_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Copilot);
}

#[test]
fn devin_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Devin);
}

#[test]
fn cursor_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Cursor);
}

#[test]
fn kimi_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Kimi);
}

#[test]
fn auggie_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Auggie);
}

#[test]
fn kiro_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Kiro);
}
#[test]
fn hermes_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Hermes);
}
#[test]
fn droid_native_delivery_survives_saved_acp_mode() {
    exercise_pending_acp_mode(ActorRuntime::Droid);
}

fn exercise_pending_acp_mode(runtime: ActorRuntime) {
    exercise_startup("Native fixture\r\n>", false, Some(runtime));
}

#[test]
fn antigravity_captured_narrow_prompt_delivers_automatically() {
    exercise_captured_startup("narrow");
}

#[test]
fn antigravity_captured_wide_prompt_delivers_automatically() {
    exercise_captured_startup("wide");
}

fn exercise_captured_startup(layout: &str) {
    // Sanitized native 1.2.2 output from the reported blocked sessions. The
    // right-aligned model status must not affect whether input is submitted.
    let frames: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/antigravity_ready_1_2_2.json"))
            .expect("captured frames");
    exercise_startup(
        frames[layout].as_str().expect("captured layout"),
        false,
        None,
    );
}

fn exercise_startup(prompt: &str, slow_start: bool, pending_acp: Option<ActorRuntime>) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("startup", "").expect("group");
    let mut actor = Actor::new("agy");
    actor.runtime = pending_acp.unwrap_or(ActorRuntime::Antigravity);
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    let script = temp.path().join("terminal.py");
    std::fs::write(temp.path().join("prompt.ansi"), prompt).expect("prompt frame");
    std::fs::write(
        &script,
        r#"
import os,sys,tty,pathlib,json,time,select
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
os.write(1,b'\x1b[?2004h'+(root/'prompt.ansi').read_bytes())
(root/'ready').touch()
# Some native TUIs enable paste mode before mounting the conversation input.
# Deliberately discard startup input, like the reported native initialization.
if sys.argv[2] == 'slow':
 deadline=time.monotonic()+0.8
 while time.monotonic()<deadline:
  if select.select([0],[],[],0.01)[0]:os.read(0,65536)
buf=b''
while True:
 data=os.read(0,65536)
 if not data:break
 buf+=data
 while b'\r' in buf:
  text,buf=buf.split(b'\r',1)
  if not text:continue
  with (root/'received').open('a') as f:f.write(json.dumps(text.decode())+'\n')
"#,
    )
    .expect("write fixture");
    let mut preamble = String::new();
    let cancelled = AtomicBool::new(false);
    for cycle in 0..2 {
        if cycle > 0 {
            std::fs::remove_file(temp.path().join("ready")).expect("clear fixture signal");
            std::fs::remove_file(temp.path().join("received")).expect("clear fixture input");
        }
        let terminal = cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command: vec![
                "python3".into(),
                script.to_string_lossy().into_owned(),
                temp.path().to_string_lossy().into_owned(),
                if slow_start { "slow" } else { "ready" }.into(),
            ],
            cwd: temp.path().to_path_buf(),
            env: BTreeMap::new(),
            cols: 120,
            rows: 40,
        })
        .expect("start fixture");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wait_for(|| temp.path().join("ready").exists());
            assert!(
                !temp.path().join("received").exists(),
                "idle startup submits no prompt"
            );
            if pending_acp.is_some() {
                let response = crate::handle_request(
                    &home,
                    &DaemonRequest {
                        v: 1,
                        op: "actor_update".into(),
                        args: serde_json::json!({
                            "group_id":group.group_id,
                            "actor_id":actor.id,
                            "runtime_mode":"acp",
                            "by":"user"
                        })
                        .as_object()
                        .expect("update arguments")
                        .clone(),
                    },
                );
                assert!(response.ok, "{:?}", response.error);
                group = store.load(&group.group_id).expect("saved ACP settings");
                actor = group.actors[0].clone();
                assert_eq!(actor.runtime_mode, cccc_contracts::RuntimeMode::Acp);
                assert_eq!(
                    cccc_runtime::status(&group.group_id, &actor.id)
                        .expect("native status")
                        .pid,
                    terminal.pid,
                    "saving mode must not replace the native terminal"
                );
                assert!(
                    !crate::ops::local_headless::running(&group.group_id, &actor.id),
                    "saving mode must not launch ACP"
                );
            }
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data =
                serde_json::json!({"to":["agy"],"text":"FIRST_TASK","message_mode":"send"})
                    .as_object()
                    .expect("event data")
                    .clone();
            let job = DeliveryJob {
                home: home.clone(),
                group: group.clone(),
                actor: actor.clone(),
                event,
                claimed: true,
                transport: "pty",
            };
            assert!(
                matches!(
                    process_batch(&mut vec![job.clone()], &mut preamble, &cancelled),
                    super::BatchOutcome::Delivered
                ),
                "first task must be delivered without a Web attachment or confirmation"
            );
            assert_eq!(
                crate::ops::runtime_delivery::latest_state(
                    &home,
                    &group.group_id,
                    &actor.id,
                    &job.event.id
                )
                .expect("delivery state")
                .expect("native receipt"),
                ("accepted".into(), "pty".into()),
                "delivery must describe the running native surface"
            );
            let mut second = job;
            second.event.id = uuid::Uuid::new_v4().simple().to_string();
            second
                .event
                .data
                .insert("text".into(), "SECOND_TASK".into());
            assert!(matches!(
                process_batch(&mut vec![second], &mut preamble, &cancelled),
                super::BatchOutcome::Delivered
            ));
            wait_for(|| {
                std::fs::read_to_string(temp.path().join("received"))
                    .is_ok_and(|text| text.lines().count() >= 2)
            });
            let lines = std::fs::read_to_string(temp.path().join("received")).expect("read input");
            let messages: Vec<String> = lines
                .lines()
                .map(|line| serde_json::from_str(line).expect("input record"))
                .collect();
            assert_eq!(
                messages.len(),
                2,
                "one submission per task after start or restart"
            );
            if actor.runtime == ActorRuntime::Antigravity {
                assert!(!messages[0].contains("MCP setup request"));
            }
            assert_eq!(messages[0].matches("[CCCC] You are agy").count(), 1);
            assert!(messages[0].contains("cccc_bootstrap"));
            assert!(messages[0].contains("FIRST_TASK"));
            assert!(messages[0].find("cccc_bootstrap") < messages[0].find("FIRST_TASK"));
            assert!(!messages[1].contains("MCP setup request"));
            assert!(!messages[1].contains("[CCCC] You are agy"));
            if actor.runtime == ActorRuntime::Antigravity {
                assert!(messages[1].contains("Otherwise continue without repeating bootstrap"));
            }
            assert!(messages[1].contains("SECOND_TASK"));
        }));
        cccc_runtime::stop(&group.group_id, &actor.id).expect("stop fixture");
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

#[test]
fn a_pty_session_hears_once_about_an_instruction_file_changed_after_launch() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let work = temp.path().join("work");
    std::fs::create_dir_all(&work).expect("workspace");
    std::fs::write(work.join("AGENTS.md"), "commit when asked").expect("instructions");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("instructions", "").expect("group");
    let mut group = cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::scope::detect(&work).expect("fixture scope"),
    )
    .expect("attach scope");
    let script = temp.path().join("terminal.py");
    std::fs::write(
        &script,
        r#"
import os,sys,tty,pathlib,json
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
(root/'ready').touch()
buf=b''
while True:
 data=os.read(0,65536)
 if not data:break
 buf+=data
 while b'\r' in buf:
  text,buf=buf.split(b'\r',1)
  if not text:continue
  with (root/'received').open('a') as f:f.write(json.dumps(text.decode())+'\n')
"#,
    )
    .expect("write fixture");
    let mut actor = Actor::new("worker");
    actor.runtime = ActorRuntime::Custom;
    actor.command = vec![
        "python3".into(),
        script.to_string_lossy().into_owned(),
        temp.path().to_string_lossy().into_owned(),
    ];
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    let response = crate::handle_request(
        &home,
        &DaemonRequest {
            v: 1,
            op: "actor_start".into(),
            args: serde_json::json!({"group_id":group.group_id,"actor_id":"worker","by":"user"})
                .as_object()
                .expect("start arguments")
                .clone(),
        },
    );
    assert!(response.ok, "{:?}", response.error);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(|| temp.path().join("ready").exists());
        // Edited after the launch recorded what the session started with.
        std::fs::write(work.join("AGENTS.md"), "commit each milestone").expect("edit");
        let group = store.load(&group.group_id).expect("group");
        let actor = group.actors[0].clone();
        let mut preamble = String::new();
        let cancelled = AtomicBool::new(false);
        for text in ["FIRST_TASK", "SECOND_TASK"] {
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data = serde_json::json!({"to":["worker"],"text":text,"message_mode":"send"})
                .as_object()
                .expect("event data")
                .clone();
            let job = DeliveryJob {
                home: home.clone(),
                group: group.clone(),
                actor: actor.clone(),
                event,
                claimed: true,
                transport: "pty",
            };
            assert!(matches!(
                process_batch(&mut vec![job], &mut preamble, &cancelled),
                super::BatchOutcome::Delivered
            ));
        }
        let submitted = |task: &str| {
            std::fs::read_to_string(temp.path().join("received"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str::<String>(line).expect("input record"))
                .find(|text| text.contains(task))
        };
        wait_for(|| submitted("SECOND_TASK").is_some());
        let first = submitted("FIRST_TASK").expect("first task");
        let second = submitted("SECOND_TASK").expect("second task");
        assert_eq!(
            first.matches("INSTRUCTION FILES CHANGED").count(),
            1,
            "{first}"
        );
        assert!(first.contains(": AGENTS.md changed."), "{first}");
        assert!(!first.contains("milestone"), "no file contents: {first}");
        assert!(!second.contains("INSTRUCTION FILES CHANGED"), "{second}");
    }));
    cccc_runtime::stop(&group.group_id, "worker").expect("stop fixture");
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[track_caller]
fn wait_for(ready: impl FnMut() -> bool) {
    wait_for_timeout(Duration::from_secs(3), ready);
}

#[track_caller]
fn wait_for_timeout(timeout: Duration, mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + timeout;
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "fixture timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_closed_startup_terminal_holds_claimed_mail_until_explicit_start() {
    use cccc_core::ledger;
    use serde_json::{Value, json};

    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("startup retry", "").expect("group");
    let mut group = cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::scope::detect(temp.path()).expect("fixture scope"),
    )
    .expect("attach scope");
    let mut actor = Actor::new("worker");
    actor.runtime = ActorRuntime::Cursor;
    actor.command = vec![
        "/bin/sh".into(),
        "-c".into(),
        "stty raw -echo; exec cat > received".into(),
    ];
    group.state = GroupState::Active;
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let old = cccc_runtime::start_actor_with_history(
        cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command: vec!["/bin/sh".into(), "-c".into(),
                "stty raw -echo; printf '\\033[?2004hWorkspace Trust Required\\r\\n'; touch prompt; dd bs=1 count=1 of=choice 2>/dev/null".into()],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        cccc_runtime::HistoryConfig {
            path: temp.path().join("terminal.pty"),
            max_bytes: 65536,
            hot_bytes: 8192,
            persist: false,
        },
        cccc_runtime::ActorLaunch { runtime: ActorRuntime::Cursor, submit: actor.submit },
    ).expect("start terminal with startup prompt");
    let call = |op: &str, extra: Value| {
        let mut args = extra.as_object().expect("arguments").clone();
        args.extend([
            ("by".into(), json!("user")),
            ("group_id".into(), json!(group.group_id)),
            ("actor_id".into(), json!(actor.id)),
        ]);
        crate::handle_request(
            &home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            },
        )
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(|| temp.path().join("prompt").exists());
        let mut mail = Event::new("chat.message", &group.group_id);
        mail.by = "user".into();
        mail.data = json!({"to":[actor.id], "message_mode":"mail", "text":"STARTUP_MAIL_ONCE"})
            .as_object()
            .expect("mail")
            .clone();
        let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
        ledger::append(&ledger_path, &mail).expect("append mail");
        let claim = call(
            "message_deliver",
            json!({"source_event_id":mail.id,"actor_ids":[actor.id]}),
        );
        assert!(claim.ok, "{:?}", claim.error);
        assert_eq!(claim.result["delivery_state"], "claimed");
        assert!(
            std::fs::read(temp.path().join("choice"))
                .unwrap_or_default()
                .is_empty(),
            "the startup prompt has not received a byte"
        );
        cccc_runtime::write(&group.group_id, &actor.id, b"q").expect("user rejects prompt");
        wait_for(|| {
            cccc_runtime::status(&group.group_id, &actor.id).is_ok_and(|status| !status.running)
        });
        assert_eq!(
            std::fs::read(temp.path().join("choice")).expect("choice"),
            b"q"
        );

        let exited = actor_runtime::reap_exited().expect("reap");
        assert!(
            exited
                .iter()
                .any(|status| status.group_id == group.group_id
                    && status.started_at == old.started_at)
        );
        assert!(
            !actor_runtime::reap_exited()
                .expect("reap again")
                .iter()
                .any(|status| status.group_id == group.group_id)
        );
        let saved = call("actor_update", json!({"patch":{"runtime":"custom"}}));
        assert!(saved.ok, "{:?}", saved.error);
        let saved_group = store.load(&group.group_id).expect("saved settings");
        assert_eq!(saved_group.actors[0].runtime, ActorRuntime::Custom);
        // The actual startup exit is observable before exit reconciliation.
        // Saving next-launch settings is not an explicit lifecycle retry.
        let automated = actor_runtime::apply(&home, &saved_group, &actor.id, "actor.start");
        assert_eq!(
            automated
                .expect_err("automatic start must wait for user")
                .code,
            "runtime_confirmation_paused"
        );
        assert!(!temp.path().join("received").exists());

        let retried = call("actor_start", json!({}));
        assert!(retried.ok, "{:?}", retried.error);
        let new = cccc_runtime::status(&group.group_id, &actor.id).expect("replacement");
        assert_ne!(new.started_at, old.started_at);
        // Custom submissions pace both the preamble and the business input.
        wait_for_timeout(Duration::from_secs(10), || {
            crate::ops::runtime_delivery::latest_state(&home, &group.group_id, &actor.id, &mail.id)
                .expect("latest delivery state")
                .is_some_and(|state| state.0 == "accepted")
        });
        wait_for(|| {
            std::fs::read_to_string(temp.path().join("received"))
                .is_ok_and(|text| text.contains("STARTUP_MAIL_ONCE"))
        });
        let stopped = call("actor_stop", json!({}));
        assert!(stopped.ok, "{:?}", stopped.error);
        let received = std::fs::read_to_string(temp.path().join("received")).expect("received");
        assert_eq!(received.matches("STARTUP_MAIL_ONCE").count(), 1);
        let stale = call("actor_terminal_ready", json!({"generation":new.started_at}));
        assert!(!stale.ok);
        assert_eq!(
            stale.error.expect("stale release").code,
            "runtime_session_changed"
        );
    }));
    crate::ops::actor_delivery::shutdown_actor(&group.group_id, &actor.id);
    let _ = cccc_runtime::stop(&group.group_id, &actor.id);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn unknown_deepseek_attempt_settles_before_any_auto_wake() {
    let temp = tempfile::tempdir().expect("isolated test directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
    home.initialize().expect("initialize fixture state");
    let store = GroupStore::new(home.clone()).expect("open isolated group store");
    let mut group = store
        .create("unknown native attempt", "")
        .expect("create fixture group");
    let mut actor = Actor::new("peer");
    actor.runtime = ActorRuntime::Deepseek;
    group.actors.push(actor.clone());
    store.save(&group).expect("save fixture group");
    let event = Event::new("chat.message", &group.group_id);
    cccc_core::ledger::append(
        &store
            .ledger_path(&group.group_id)
            .expect("resolve group ledger"),
        &event,
    )
    .expect("append fixture event");
    crate::ops::runtime_delivery::claim(&home, &group, &actor, &event.id, "deepseek", false)
        .expect("claim fixture delivery");
    let mut attempted = HashSet::from([event.id.clone()]);
    let mut jobs = vec![DeliveryJob {
        home: home.clone(),
        group: group.clone(),
        actor: actor.clone(),
        event: event.clone(),
        transport: "deepseek",
        claimed: true,
    }];
    let mut wakes = WakeBudget::default();
    assert_eq!(
        process_deepseek_batch(
            &mut jobs,
            &home,
            &group,
            &actor,
            &AtomicBool::new(false),
            &mut attempted,
            &mut wakes
        ),
        BatchOutcome::Delivered
    );
    assert!(jobs.is_empty());
    assert_eq!(
        wakes.spent, 0,
        "unknown work must be resolved before restarting the provider"
    );
    assert_eq!(
        crate::ops::runtime_delivery::latest_state(&home, &group.group_id, &actor.id, &event.id)
            .expect("read latest delivery state")
            .expect("settled delivery state")
            .0,
        "ambiguous"
    );
}
