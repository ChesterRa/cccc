//! Offline provider protocol and isolated launch state; no real calls or login.
use super::*;
use cccc_contracts::{ActorRuntime, CodexVoiceAnalystSettings, RuntimeMode};
use serde_json::{Value, json};
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, time::Duration};

pub(super) fn isolated_child(name: &str) -> bool {
    if std::env::var_os("CCCC_TEST_VOICE_ACP_CHILD").is_some() {
        return false;
    }
    let temp = tempfile::tempdir().expect("child environment");
    let launcher = temp.path().join("cccc");
    std::fs::write(&launcher, "#!/bin/sh\nexit 1\n").expect("launcher");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--nocapture"])
        .env("CCCC_TEST_VOICE_ACP_CHILD", "1")
        .env("CCCC_LAUNCHER_PATH", launcher)
        .env("HOME", temp.path().join("provider-home"))
        .env("CODEX_HOME", temp.path().join("codex"))
        .env("GROK_HOME", temp.path().join("grok"))
        .env_remove("GROK_AUTH_PATH")
        .env_remove("CCCC_CODEX_AUTH_PATH")
        .output()
        .expect("fixture child");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

pub(super) async fn fixture() -> (
    tempfile::TempDir,
    HomeLayout,
    CodexVoiceSessions,
    CodexVoiceAnalystSettings,
    BTreeMap<String, String>,
) {
    fixture_with_source(include_str!(
        "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
    ))
    .await
}

async fn fixture_with_source(
    source: &str,
) -> (
    tempfile::TempDir,
    HomeLayout,
    CodexVoiceSessions,
    CodexVoiceAnalystSettings,
    BTreeMap<String, String>,
) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    home.initialize().expect("initialize");
    let program = temp.path().join("devin");
    std::fs::write(&program, source).expect("protocol fixture");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("executable");
    let settings = CodexVoiceAnalystSettings {
        runtime: ActorRuntime::Devin,
        runtime_mode: RuntimeMode::Acp,
        command: vec![program.to_string_lossy().into_owned()],
        ..Default::default()
    };
    let env = BTreeMap::from([
        (
            "HOME".into(),
            temp.path().join("provider").to_string_lossy().into_owned(),
        ),
        (
            "CODEX_HOME".into(),
            temp.path().join("codex").to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            temp.path()
                .join("provider/config")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            temp.path()
                .join("provider/data")
                .to_string_lossy()
                .into_owned(),
        ),
    ]);
    cccc_core::codex_voice_settings::save(&home, &settings).expect("settings");
    cccc_core::codex_voice_settings::replace_private_environment(&home, &env).expect("env");
    let runtime = Arc::new(
        persistence::launch_analyst(&home)
            .await
            .expect("warm Analyst"),
    );
    runtime.start_monitor(home.clone());
    persistence::persist_analyst(&home, &runtime, false).expect("empty receipt");
    let sessions = CodexVoiceSessions::default();
    sessions.state.lock().await.analyst = Some(runtime);
    (temp, home, sessions, settings, env)
}

#[tokio::test(flavor = "current_thread")]
async fn reset_replacement_failures_report_pending_and_keep_owners_recoverable() {
    if isolated_child(
        "codex_voice::acp_regression_tests::reset_replacement_failures_report_pending_and_keep_owners_recoverable",
    ) {
        return;
    }
    for fail_persistence in [false, true] {
        let (temp, home, sessions, _, _) = fixture().await;
        let sessions = Arc::new(sessions);
        let old = sessions
            .state
            .lock()
            .await
            .analyst
            .clone()
            .expect("old owner");
        let generation = old.analyst.generation().to_owned();
        let state_path = home.daemon_dir().join(persistence::TEST_ANALYST_STATE_FILE);
        if fail_persistence {
            std::fs::remove_file(&state_path).expect("remove fixture receipt");
            std::fs::create_dir(&state_path).expect("block fixture receipt write");
        } else {
            std::fs::remove_file(temp.path().join("devin")).expect("remove fake executable");
        }
        let result = sessions.reset_analyst(&home, &generation).await;
        let pending = match result {
            Err(error) => {
                error.downcast_ref::<super::restart::VoiceGateError>()
                    == Some(&super::restart::VoiceGateError::ResetRecoveryPending)
            }
            Ok(_) => false,
        };
        if fail_persistence {
            let retained = sessions
                .state
                .lock()
                .await
                .analyst
                .clone()
                .expect("new owner retained");
            assert_ne!(retained.analyst.generation(), generation);
            std::fs::remove_dir(&state_path).expect("restore fixture receipt path");
        }
        sessions
            .recover_before_start()
            .await
            .expect("cleanup retry");
        assert!(sessions.state.lock().await.analyst.is_none());
        assert!(pending, "post-admission reset failure must report pending");
        if fail_persistence {
            // Cleanup must finish before a later launch is allowed to own a new process.
            let next = persistence::launch_fresh_analyst(&home, String::new())
                .await
                .expect("new owner after recovery");
            next.analyst.shutdown().await.expect("stop new owner");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn slow_analyst_cleanup_does_not_block_a_single_thread_web_runtime() {
    if isolated_child(
        "codex_voice::acp_regression_tests::slow_analyst_cleanup_does_not_block_a_single_thread_web_runtime",
    ) {
        return;
    }
    let source = include_str!(
        "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
    );
    let marker = "session_id = \"\"";
    assert!(source.contains(marker));
    let injection = r#"
import signal, time
def slow_stop(signum, frame):
    Path(os.environ["HOME"]).joinpath("fixture-term").write_text("term")
    time.sleep(6)
signal.signal(signal.SIGTERM, slow_stop)
"#;
    let source = format!(
        "{}\nimport time\nwhile True:\n    time.sleep(1)\n",
        source.replacen(marker, &format!("{injection}\n{marker}"), 1)
    );
    let (temp, _home, sessions, _, _) = fixture_with_source(&source).await;
    let sessions = Arc::new(sessions);
    let request_sessions = Arc::clone(&sessions);
    let request = tokio::spawn(async move { request_sessions.prepare_web_restart().await });
    let marker_path = temp.path().join("provider/fixture-term");
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let observer = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if marker_path.exists() {
                let _ = sender.send(std::time::Instant::now());
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let marked_at = tokio::time::timeout(Duration::from_secs(10), receiver)
        .await
        .expect("old Analyst did not receive stop")
        .expect("stop observer");
    let delay = marked_at.elapsed();
    observer.join().expect("stop observer exits");
    request.await.expect("cleanup worker").expect("cleanup");
    assert!(
        delay < Duration::from_secs(1),
        "Web runtime stalled for {delay:?}"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn timed_out_cleanup_can_target_its_analyst_without_releasing_the_gate() {
    if isolated_child(
        "codex_voice::acp_regression_tests::timed_out_cleanup_can_target_its_analyst_without_releasing_the_gate",
    ) {
        return;
    }
    let source = include_str!(
        "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
    );
    let marker = "session_id = \"\"";
    assert!(source.contains(marker));
    let injection = r#"
import signal, time
Path(os.environ["HOME"]).mkdir(parents=True, exist_ok=True)
Path(os.environ["HOME"]).joinpath("fixture-pid").write_text(str(os.getpid()))
signal.signal(signal.SIGTERM, signal.SIG_IGN)
"#;
    let source = format!(
        "{}\nimport time\nwhile True:\n    time.sleep(1)\n",
        source.replacen(marker, &format!("{injection}\n{marker}"), 1)
    );
    let (temp, home, sessions, _, _) = fixture_with_source(&source).await;
    let sessions = Arc::new(sessions);
    let analyst = sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("fixture Analyst");
    let pid: u32 = std::fs::read_to_string(temp.path().join("provider/fixture-pid"))
        .expect("fake PID")
        .parse()
        .expect("PID");

    let context = cccc_contracts::codex_voice::VoiceApplicationContext::new_with_mode(
        "fixture".into(),
        "local test".into(),
        cccc_contracts::codex_voice::VoiceCallMode::Persona,
    )
    .expect("persona");
    let call = Arc::new(
        CodexVoiceCall::start(&home, None, Some(context))
            .await
            .expect("local recording lease"),
    );
    let session = Arc::new(ActiveSession {
        verbosity: Default::default(),
        notification_paused: tokio::sync::watch::channel(false).0,
        call,
        analyst: None,
        client_session_id: "fixture".into(),
        offer_digest: [0; 32],
        answer_sdp: String::new(),
        provider_connection: None,
        voice: "cove".into(),
        connection_state: AtomicU8::new(CONNECTION_UNATTACHED),
    });
    sessions.state.lock().await.active = Some(session);
    let lock_path = home
        .root()
        .join("state/voice_secretary_recording_lease.json.lock");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .expect("fixture lock");
    fs2::FileExt::lock_exclusive(&lock).expect("hold fixture lock");
    let (release, wait) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _ = wait.recv_timeout(Duration::from_secs(8));
        fs2::FileExt::unlock(&lock).expect("unlock fixture");
    });

    sessions.reserve_cleanup(false).expect("reserve cleanup");
    assert_eq!(
        sessions
            .cleanup_reserved_with_timeout(super::restart::PREPARED, Duration::from_millis(300))
            .await,
        Err(super::restart::VoiceGateError::RecoveryPending)
    );
    assert!(
        sessions
            .state
            .lock()
            .await
            .analyst
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &analyst)),
        "the old owner remains available for targeted termination"
    );
    assert_eq!(
        sessions.recover_before_start().await,
        Err(super::restart::VoiceGateError::RecoveryPending),
        "the blocking cleanup still holds the gate"
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let exited = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .map(|stat| {
                    stat.rsplit_once(')')
                        .and_then(|(_, tail)| tail.split_whitespace().next())
                        .is_some_and(|state| matches!(state, "Z" | "X"))
                })
                .unwrap_or(true);
            if exited {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("targeted Analyst process did not exit");
    release.send(()).expect("release fixture lock");
    holder.join().expect("lock holder exits");
    let cleanup = tokio::time::timeout(Duration::from_secs(5), sessions.cleanup_gate.lock())
        .await
        .expect("blocking cleanup finishes");
    drop(cleanup);
    sessions
        .recover_before_start()
        .await
        .expect("recovery can finish after cleanup");
}

#[tokio::test]
async fn empty_acp_settings_replacement_and_rollback_do_not_load_unpersisted_sessions() {
    if isolated_child(
        "codex_voice::acp_regression_tests::empty_acp_settings_replacement_and_rollback_do_not_load_unpersisted_sessions",
    ) {
        return;
    }
    let (temp, home, sessions, settings, env) = fixture().await;
    let old_id = sessions
        .state
        .lock()
        .await
        .analyst
        .as_ref()
        .expect("Analyst")
        .analyst
        .thread_id()
        .to_owned();
    let profiles = cccc_core::profiles::ProfileStore::new(home.clone()).expect("profiles");
    profiles.upsert(json!({"id":"ap_fixture", "name":"ACP fixture", "runtime":"devin", "runtime_mode":"acp", "command":settings.command}).as_object().expect("profile").clone(), None).expect("profile");
    profiles
        .replace_secrets("ap_fixture", env)
        .expect("profile env");
    let linked = CodexVoiceAnalystSettings {
        profile_id: "ap_fixture".into(),
        ..Default::default()
    };
    let replaced = sessions
        .apply_analyst_settings(&home, linked.clone(), BTreeMap::new(), vec![], false, false)
        .await
        .expect("empty custom to equivalent Profile");
    assert!(replaced.restarted);
    assert!(!replaced.started_new_session);
    assert_ne!(
        sessions
            .state
            .lock()
            .await
            .analyst
            .as_ref()
            .expect("replacement")
            .analyst
            .thread_id(),
        old_id
    );
    let broken = CodexVoiceAnalystSettings {
        command: vec![
            temp.path()
                .join("missing/devin")
                .to_string_lossy()
                .into_owned(),
        ],
        ..settings.clone()
    };
    assert!(
        sessions
            .apply_analyst_settings(&home, broken, BTreeMap::new(), vec![], false, false)
            .await
            .is_err()
    );
    assert!(
        sessions.state.lock().await.analyst.is_some(),
        "failed replacement restores an empty Analyst"
    );
    assert_eq!(
        cccc_core::codex_voice_settings::load(&home).expect("restored settings"),
        linked
    );
    let workdir = cccc_core::codex_voice_settings::workdir(&home).expect("workdir");
    let frames = std::fs::read_to_string(workdir.join("fixture_requests.jsonl")).expect("frames");
    assert!(
        !frames.contains("session/load"),
        "empty sessions are not persisted by the provider"
    );
    assert!(
        !frames.contains("session/prompt"),
        "settings must not materialize through a paid task"
    );
    let runtime = sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("restored Analyst");
    runtime
        .analyst
        .submit_input("materialize", "offline materialization fixture")
        .await
        .expect("fixture input");
    tokio::time::timeout(Duration::from_secs(4), async {
        while !runtime
            .info()
            .last_result
            .contains("offline materialization fixture")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture completed");
    let materialized_id = runtime.analyst.thread_id().to_owned();
    let replaced = sessions
        .apply_analyst_settings(&home, settings, BTreeMap::new(), vec![], false, false)
        .await
        .expect("equivalent custom setting resumes materialized session");
    assert!(replaced.restarted);
    assert!(!replaced.started_new_session);
    assert_eq!(
        sessions
            .state
            .lock()
            .await
            .analyst
            .as_ref()
            .expect("resumed Analyst")
            .analyst
            .thread_id(),
        materialized_id
    );
    sessions.shutdown().await.expect("cleanup");
}

#[tokio::test]
async fn warm_acp_console_retains_pre_admission_and_admitted_failure_details() {
    if isolated_child(
        "codex_voice::acp_regression_tests::warm_acp_console_retains_pre_admission_and_admitted_failure_details",
    ) {
        return;
    }
    let (_temp, home, sessions, _settings, _env) = fixture().await;
    let runtime = sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("Analyst");
    assert!(
        sessions.state.lock().await.active.is_none(),
        "outside a call"
    );
    for (id, text, expected, partial) in [
        (
            "rejected",
            "REJECT_BEFORE_RECEIPT",
            "Synthetic admission failure",
            false,
        ),
        (
            "failed",
            "FAIL_AFTER_RECEIPT",
            "Synthetic provider failure",
            true,
        ),
    ] {
        runtime
            .analyst
            .submit_input(id, text)
            .await
            .expect("host queue accepted");
        tokio::time::timeout(Duration::from_secs(4), async {
            while !runtime.info().last_error.contains(expected) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{id} diagnostic was not projected: {:?}", runtime.info()));
        let info = runtime.info();
        assert_eq!(info.phase, "needs_attention");
        assert_eq!(!info.last_result.is_empty(), partial);
    }
    runtime
        .analyst
        .submit_input("success", "fresh fixture result")
        .await
        .expect("next input");
    tokio::time::timeout(Duration::from_secs(4), async {
        while !runtime.info().last_result.contains("fresh fixture result") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("next success");
    assert!(runtime.info().last_error.is_empty());
    let frames = std::fs::read_to_string(
        cccc_core::codex_voice_settings::workdir(&home)
            .expect("workdir")
            .join("fixture_requests.jsonl"),
    )
    .expect("frames");
    let prompts: Vec<Value> = frames
        .lines()
        .map(|line| serde_json::from_str(line).expect("frame"))
        .filter(|frame: &Value| frame["method"] == "session/prompt")
        .collect();
    assert_eq!(prompts.len(), 3, "failures must not be replayed");
    sessions.shutdown().await.expect("cleanup");
}

async fn local_assistant_call(
    home: &HomeLayout,
    sessions: &CodexVoiceSessions,
    runtime: &Arc<AnalystRuntime>,
    attached: bool,
) -> Arc<ActiveSession> {
    let context = cccc_contracts::codex_voice::VoiceApplicationContext::new(
        "fixture-host".into(),
        "Keep the host's expression policy for this investigation.".into(),
    )
    .expect("host context");
    let call = Arc::new(
        CodexVoiceCall::start(home, Some(runtime.analyst()), Some(context))
            .await
            .expect("fixture lease"),
    );
    let session = Arc::new(ActiveSession {
        provider_connection: None,
        verbosity: Default::default(),
        notification_paused: tokio::sync::watch::channel(false).0,
        call,
        analyst: Some(Arc::clone(runtime)),
        client_session_id: "manual-fixture".into(),
        offer_digest: [0; 32],
        answer_sdp: String::new(),
        voice: "cove".into(),
        connection_state: AtomicU8::new(if attached {
            CONNECTION_ATTACHED
        } else {
            CONNECTION_UNATTACHED
        }),
    });
    sessions.state.lock().await.active = Some(Arc::clone(&session));
    session
}

async fn input(
    sessions: &CodexVoiceSessions,
    runtime: &AnalystRuntime,
    id: &str,
    text: &str,
    call: Option<&str>,
) -> anyhow::Result<()> {
    sessions
        .structured_control(
            runtime.analyst.generation(),
            "input",
            id,
            call,
            text,
            "",
            false,
            Value::Null,
        )
        .await
}

async fn settled(runtime: &AnalystRuntime, id: &str) -> ManualAnalystTask {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Some(task) = runtime.info().manual_tasks.into_iter().find(|task| {
                task.id == format!("manual-input:{id}")
                    && !matches!(task.status.as_str(), "queued" | "working")
            }) {
                return task;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("manual task settled")
}

#[tokio::test]
async fn manual_investigations_keep_exact_call_scope_and_at_most_once_outcomes() {
    if isolated_child(
        "codex_voice::acp_regression_tests::manual_investigations_keep_exact_call_scope_and_at_most_once_outcomes",
    ) {
        return;
    }
    let (_temp, home, sessions, _, _) = fixture().await;
    let runtime = sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("Analyst");
    input(&sessions, &runtime, "local", "standalone fixture", None)
        .await
        .expect("standalone");
    let standalone = settled(&runtime, "local").await;
    assert!(standalone.call_generation.is_none());
    let first = local_assistant_call(&home, &sessions, &runtime, true).await;
    let gen_a = first.call.generation();
    assert!(runtime.manual_results_for_call(gen_a).is_empty());
    let investigation = format!("original investigation {}", "日本語の調査".repeat(80));
    input(&sessions, &runtime, "one", &investigation, Some(gen_a))
        .await
        .expect("bound input");
    input(&sessions, &runtime, "one", &investigation, Some(gen_a))
        .await
        .expect("same request retry");
    assert!(
        input(
            &sessions,
            &runtime,
            "one",
            "different investigation",
            Some(gen_a)
        )
        .await
        .is_err()
    );
    let task = settled(&runtime, "one").await;
    assert_eq!(task.status, "completed");
    assert!(task.result.contains("Context ID: fixture-host"));
    assert!(
        task.result.contains(&investigation),
        "Analyst receives the full task"
    );
    assert!(!standalone.result.contains("Context ID: fixture-host"));
    let outputs = runtime.manual_results_for_call(gen_a);
    assert_eq!(outputs.len(), 1);
    let command = &outputs[0].1;
    assert_eq!(command["type"], "session.context.append");
    assert!(command.get("delegation_item_id").is_none());
    let data: Value = serde_json::from_str(
        command["content"][0]["text"]
            .as_str()
            .expect("context text")
            .split_once('\n')
            .expect("task data")
            .1,
    )
    .expect("valid Unicode task data");
    assert_eq!(data["task_abridged"], true);
    assert!(data["task"].as_str().expect("bounded label").len() <= 1024);
    assert_eq!(data["result"], task.result);
    assert!(
        command["content"][0]["text"]
            .as_str()
            .expect("context")
            .contains("original investigation")
    );
    runtime.manual_result_projected(&outputs[0].0);
    assert!(runtime.manual_results_for_call(gen_a).is_empty());
    for index in 0..10 {
        let id = format!("unreturned-{index}");
        input(
            &sessions,
            &runtime,
            &id,
            "awaiting original call output",
            Some(gen_a),
        )
        .await
        .expect("bound task");
        settled(&runtime, &id).await;
    }
    let waiting = runtime.manual_results_for_call(gen_a);
    assert_eq!(
        waiting.len(),
        10,
        "UI history limits cannot discard pending call output"
    );
    for (id, _) in waiting {
        runtime.manual_result_projected(&id);
    }
    input(
        &sessions,
        &runtime,
        "late",
        "HOLD original-call task",
        Some(gen_a),
    )
    .await
    .expect("held task");
    tokio::time::timeout(Duration::from_secs(4), async {
        while !runtime
            .info()
            .manual_tasks
            .iter()
            .any(|task| task.id == "manual-input:late" && task.status == "working")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("held task started");
    sessions.stop(gen_a).await.expect("end call A");
    let second = local_assistant_call(&home, &sessions, &runtime, true).await;
    assert!(
        input(
            &sessions,
            &runtime,
            "stale",
            "must not execute",
            Some(gen_a)
        )
        .await
        .is_err()
    );
    assert!(
        input(
            &sessions,
            &runtime,
            "one",
            "original investigation",
            Some(second.call.generation())
        )
        .await
        .is_err()
    );
    input(
        &sessions,
        &runtime,
        "queued",
        "queued behind held task",
        Some(second.call.generation()),
    )
    .await
    .expect("queue");
    assert!(
        runtime
            .analyst
            .cancel_current()
            .await
            .expect("cancel held and queued")
    );
    assert_eq!(settled(&runtime, "late").await.status, "cancelled");
    assert_eq!(settled(&runtime, "queued").await.status, "cancelled");
    let outputs = runtime.manual_results_for_call(second.call.generation());
    assert_eq!(
        outputs.len(),
        1,
        "only this call's queued cancellation returns"
    );
    assert!(
        runtime.manual_results_for_call(gen_a).is_empty(),
        "retired call cannot return late work"
    );
    for (id, text) in [
        ("rejected", "REJECT_BEFORE_RECEIPT"),
        ("failure", "FAIL_AFTER_RECEIPT"),
    ] {
        input(
            &sessions,
            &runtime,
            id,
            text,
            Some(second.call.generation()),
        )
        .await
        .expect("queue accepted");
        let task = settled(&runtime, id).await;
        assert_eq!(task.status, "failed");
        assert!(!task.error.is_empty());
        let outputs = runtime.manual_results_for_call(second.call.generation());
        let output = outputs
            .iter()
            .find(|(key, _)| key == &task.id)
            .expect("failure returned");
        assert!(
            !output.1["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("live:"),
            "partial output is not a finished answer"
        );
        runtime.manual_result_projected(&task.id);
    }
    sessions
        .stop(second.call.generation())
        .await
        .expect("end B");
    // Evict old display rows, then prove lifecycle tombstones still prevent execution.
    for index in 0..10 {
        let id = format!("recent-{index}");
        input(&sessions, &runtime, &id, "recent standalone result", None)
            .await
            .expect("recent");
        settled(&runtime, &id).await;
    }
    assert_eq!(runtime.info().manual_tasks.len(), 8);
    assert!(
        input(&sessions, &runtime, "local", "standalone fixture", None)
            .await
            .is_err()
    );
    let frames = std::fs::read_to_string(
        cccc_core::codex_voice_settings::workdir(&home)
            .expect("workdir")
            .join("fixture_requests.jsonl"),
    )
    .expect("frames");
    let prompts: Vec<Value> = frames
        .lines()
        .map(|line| serde_json::from_str(line).expect("frame"))
        .filter(|frame: &Value| frame["method"] == "session/prompt")
        .collect();
    assert_eq!(
        prompts.len(),
        25,
        "retry, queued cancellation and stale scopes must not execute"
    );
    sessions.shutdown().await.expect("cleanup");
}

#[tokio::test]
async fn manual_result_traverses_the_actual_voice_socket_without_provider_delegation() {
    if isolated_child(
        "codex_voice::acp_regression_tests::manual_result_traverses_the_actual_voice_socket_without_provider_delegation",
    ) {
        return;
    }
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    let (_temp, home, sessions, _, _) = fixture().await;
    let runtime = sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("Analyst");
    let (shutdown, _) = tokio::sync::broadcast::channel(1);
    let (router, _, _, state) = crate::app_with_shutdown(
        home.clone(),
        shutdown.clone(),
        crate::WebMode::Normal,
        None,
        crate::LiveBinding {
            host: "127.0.0.1".into(),
            port: 0,
        },
        "manual-fixture".into(),
    );
    state.codex_voice.state.lock().await.analyst = Some(Arc::clone(&runtime));
    let session = local_assistant_call(&home, &state.codex_voice, &runtime, false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Web listener");
    let address = listener.local_addr().expect("address");
    let web = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("fixture Web");
    });
    let store = cccc_core::access_tokens::AccessTokenStore::new(home.clone()).expect("tokens");
    let owner = store
        .create("fixture owner", vec![], true, None)
        .expect("owner");
    let mut request = format!(
        "ws://{address}/api/v1/codex_voice/calls/{}/events",
        session.call.generation()
    )
    .into_client_request()
    .expect("request");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", owner.token).parse().expect("auth"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("socket");
    let accepted: Value = reqwest::Client::new()
        .post(format!("http://{address}/api/v1/codex_voice/analysts/{}/control", runtime.analyst.generation()))
        .bearer_auth(&owner.token)
        .json(&json!({"action":"input", "input_id":"roundtrip", "text":"offline weather fixture", "call_generation":session.call.generation()}))
        .send().await.expect("control HTTP").json().await.expect("control payload");
    assert_eq!(accepted["ok"], true, "{accepted:?}");
    assert_eq!(accepted["result"]["accepted"], true);
    assert_eq!(
        accepted["result"]["analyst"]["manual_tasks"][0]["text"],
        "offline weather fixture"
    );
    let returned = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(Message::Text(frame))) = socket.next().await {
            let value: Value = serde_json::from_str(&frame).expect("frame");
            if value["type"] == "provider_command"
                && value["message"]["content"][0]["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("offline weather fixture"))
            {
                return value;
            }
        }
        panic!("missing manual outcome");
    })
    .await
    .expect("roundtrip timeout");
    assert_eq!(returned["message"]["type"], "session.context.append");
    let context = returned["message"]["content"][0]["text"]
        .as_str()
        .expect("context");
    let data: Value =
        serde_json::from_str(context.split_once('\n').expect("context data").1).expect("task data");
    assert_eq!(data["status"], "completed");
    assert!(data["result"].as_str().expect("result").contains("live:"));
    assert!(
        data["result"]
            .as_str()
            .expect("result")
            .contains("offline weather fixture")
    );
    assert!(returned["message"].get("delegation_item_id").is_none());
    assert!(
        runtime
            .manual_results_for_call(session.call.generation())
            .is_empty()
    );
    socket.close(None).await.expect("close");
    state.codex_voice.shutdown().await.expect("cleanup");
    sessions.state.lock().await.analyst = None;
    web.abort();
    let _ = web.await;
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_settings_replacement_does_not_let_web_restart_pass_a_live_owner() {
    if isolated_child(
        "codex_voice::acp_regression_tests::cancelled_settings_replacement_does_not_let_web_restart_pass_a_live_owner",
    ) {
        return;
    }
    let source = slow_replacement_source();
    let (temp, home, sessions, mut settings, _) = fixture_with_source(&source).await;
    let sessions = Arc::new(sessions);
    let pid: u32 = std::fs::read_to_string(temp.path().join("provider/fixture-pid"))
        .expect("fixture pid")
        .parse()
        .expect("pid");
    settings
        .command
        .extend(["--permission-mode".into(), "smart".into()]);
    let work_sessions = Arc::clone(&sessions);
    let work_home = home.clone();
    let task = tokio::spawn(async move {
        work_sessions
            .apply_analyst_settings(
                &work_home,
                settings,
                BTreeMap::new(),
                Vec::new(),
                false,
                false,
            )
            .await
    });
    let term = temp.path().join("provider/fixture-term");
    tokio::time::timeout(Duration::from_secs(12), async {
        while !term.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("old process signalled");
    task.abort();
    let _ = task.await;
    let barrier_sessions = Arc::clone(&sessions);
    let mut barrier = tokio::spawn(async move { barrier_sessions.prepare_web_restart().await });
    match tokio::time::timeout(Duration::from_millis(250), &mut barrier).await {
        Ok(Ok(result)) => assert!(
            result.is_err() || !fixture_pid_alive(pid),
            "restart barrier accepted while cancelled settings replacement still had a live fixture Analyst"
        ),
        Ok(Err(error)) => panic!("restart task failed: {error}"),
        Err(_) => {
            let result = tokio::time::timeout(Duration::from_secs(15), barrier)
                .await
                .expect("restart barrier finished")
                .expect("restart task");
            assert!(
                result.is_err() || !fixture_pid_alive(pid),
                "restart barrier completed before the old fixture Analyst exited"
            );
        }
    }
    sessions.shutdown().await.expect("finish fixture cleanup");
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_start_replacement_does_not_let_web_restart_pass_a_live_owner() {
    if isolated_child(
        "codex_voice::acp_regression_tests::cancelled_start_replacement_does_not_let_web_restart_pass_a_live_owner",
    ) {
        return;
    }
    let source = slow_replacement_source();
    let (temp, home, sessions, _, _) = fixture_with_source(&source).await;
    let sessions = Arc::new(sessions);
    let pid: u32 = std::fs::read_to_string(temp.path().join("provider/fixture-pid"))
        .expect("fixture pid")
        .parse()
        .expect("pid");
    {
        let state = sessions.state.lock().await;
        state
            .analyst
            .as_ref()
            .expect("old owner")
            .mark_failed("analyst_disconnected");
    }
    let config = RealtimeCallConfig {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        grok_executable: None,
        auth_path: temp.path().join("missing-auth.json"),
        base_url: "http://127.0.0.1:1".into(),
        voice: "cove".into(),
        preferences: Default::default(),
        application_context: None,
    };
    let work_sessions = Arc::clone(&sessions);
    let work_home = home.clone();
    let task = tokio::spawn(async move {
        work_sessions
            .start_with_realtime(&work_home, "fixture-retry", "offer", config)
            .await
    });
    let term = temp.path().join("provider/fixture-term");
    tokio::time::timeout(Duration::from_secs(12), async {
        while !term.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("old process signalled");
    task.abort();
    let _ = task.await;
    let barrier_sessions = Arc::clone(&sessions);
    let mut barrier = tokio::spawn(async move { barrier_sessions.prepare_web_restart().await });
    match tokio::time::timeout(Duration::from_millis(250), &mut barrier).await {
        Ok(Ok(result)) => assert!(
            result.is_err() || !fixture_pid_alive(pid),
            "restart barrier accepted while cancelled Start replacement still had a live fixture Analyst"
        ),
        Ok(Err(error)) => panic!("restart task failed: {error}"),
        Err(_) => {
            let result = tokio::time::timeout(Duration::from_secs(15), barrier)
                .await
                .expect("restart barrier finished")
                .expect("restart task");
            assert!(
                result.is_err() || !fixture_pid_alive(pid),
                "restart barrier completed before the old fixture Analyst exited"
            );
        }
    }
    sessions.shutdown().await.expect("finish fixture cleanup");
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_after_cancelled_start_reuses_replacement_without_owner_overlap() {
    if isolated_child(
        "codex_voice::acp_regression_tests::retry_after_cancelled_start_reuses_replacement_without_owner_overlap",
    ) {
        return;
    }
    let source = slow_replacement_source();
    let (temp, home, sessions, _, _) = fixture_with_source(&source).await;
    let sessions = Arc::new(sessions);
    {
        let state = sessions.state.lock().await;
        state
            .analyst
            .as_ref()
            .expect("old owner")
            .mark_failed("analyst_disconnected");
    }
    let config = RealtimeCallConfig {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        grok_executable: None,
        auth_path: temp.path().join("missing-auth.json"),
        base_url: "http://127.0.0.1:1".into(),
        voice: "cove".into(),
        preferences: Default::default(),
        application_context: None,
    };
    let first_sessions = Arc::clone(&sessions);
    let first_home = home.clone();
    let first_config = config.clone();
    let first = tokio::spawn(async move {
        first_sessions
            .start_with_realtime(&first_home, "fixture-first", "offer", first_config)
            .await
    });
    let term = temp.path().join("provider/fixture-term");
    tokio::time::timeout(Duration::from_secs(12), async {
        while !term.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("old process signalled");
    first.abort();
    let _ = first.await;
    let second_sessions = Arc::clone(&sessions);
    let second_home = home.clone();
    let second = tokio::spawn(async move {
        second_sessions
            .start_with_realtime(&second_home, "fixture-second", "offer", config)
            .await
    });
    let retry = tokio::time::timeout(Duration::from_secs(15), second)
        .await
        .expect("retry finished")
        .expect("retry task");
    assert!(
        retry.is_err(),
        "fixture deliberately lacks real-time credentials"
    );
    let overlap = std::fs::read_to_string(temp.path().join("provider/fixture-overlap"))
        .expect("replacement started");
    let spawns =
        std::fs::read_to_string(temp.path().join("provider/fixture-spawns")).expect("spawn record");
    assert_eq!(
        overlap, "0",
        "replacement started while old Analyst was alive"
    );
    assert_eq!(
        spawns.lines().count(),
        2,
        "retry started a second replacement instead of reusing the first"
    );
    sessions.shutdown().await.expect("finish fixture cleanup");
}

#[cfg(target_os = "linux")]
fn fixture_pid_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .is_some_and(|stat| {
            stat.rsplit_once(')')
                .map(|(_, rest)| !matches!(rest.trim_start().chars().next(), Some('Z' | 'X')))
                .unwrap_or(false)
        })
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_does_not_open_a_call_after_web_restart_was_reserved() {
    if isolated_child(
        "codex_voice::acp_regression_tests::start_does_not_open_a_call_after_web_restart_was_reserved",
    ) {
        return;
    }
    let source = slow_replacement_source();
    let (temp, home, sessions, _, _) = fixture_with_source(&source).await;
    let sessions = Arc::new(sessions);
    {
        let state = sessions.state.lock().await;
        state
            .analyst
            .as_ref()
            .expect("old owner")
            .mark_failed("analyst_disconnected");
    }
    let config = RealtimeCallConfig {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        grok_executable: None,
        auth_path: temp.path().join("missing-auth.json"),
        base_url: "http://127.0.0.1:1".into(),
        voice: "cove".into(),
        preferences: Default::default(),
        application_context: None,
    };
    let starter = Arc::clone(&sessions);
    let start_home = home.clone();
    let start = tokio::spawn(async move {
        starter
            .start_with_realtime(&start_home, "fixture", "offer", config)
            .await
    });
    let term = temp.path().join("provider/fixture-term");
    tokio::time::timeout(Duration::from_secs(12), async {
        while !term.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("old process signalled");
    let restarter = Arc::clone(&sessions);
    let restart = tokio::spawn(async move { restarter.prepare_web_restart().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while sessions.restart_phase.load(Ordering::Acquire) == super::restart::OPEN {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("restart reserved");
    let start_result = tokio::time::timeout(Duration::from_secs(15), start)
        .await
        .expect("start finished")
        .expect("start task");
    let rejected = match start_result {
        Err(error) => {
            error.downcast_ref::<super::restart::VoiceGateError>()
                == Some(&super::restart::VoiceGateError::Restarting)
        }
        Ok(_) => false,
    };
    assert!(
        rejected,
        "Start must recheck the restart phase before opening the call"
    );
    assert!(
        sessions.state.lock().await.active.is_none(),
        "no real-time call was created"
    );
    let _ = tokio::time::timeout(Duration::from_secs(15), restart)
        .await
        .expect("restart finished")
        .expect("restart task");
    sessions.shutdown().await.expect("finish fixture cleanup");
}

#[cfg(target_os = "linux")]
fn slow_replacement_source() -> String {
    let source = include_str!(
        "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
    );
    let marker = "session_id = \"\"";
    assert!(source.contains(marker));
    let injection = r#"
import signal, time
folder=Path(os.environ["HOME"])
folder.mkdir(parents=True, exist_ok=True)
first=folder.joinpath("fixture-first-pid")
if first.exists():
    prior=int(first.read_text())
    try:
        stat=Path(f"/proc/{prior}/stat").read_text()
        alive=stat[stat.rfind(")")+2:].split()[0] not in ("Z", "X")
    except FileNotFoundError:
        alive=False
    folder.joinpath("fixture-overlap").write_text("1" if alive else "0")
else:
    first.write_text(str(os.getpid()))
folder.joinpath("fixture-pid").write_text(str(os.getpid()))
with folder.joinpath("fixture-spawns").open("a") as f:
    f.write(str(os.getpid())+"\n")
def slow_stop(signum, frame):
    folder.joinpath("fixture-term").write_text("term")
    time.sleep(6)
signal.signal(signal.SIGTERM, slow_stop)
"#;
    format!(
        "{}\nimport time\nwhile True:\n    time.sleep(1)\n",
        source.replacen(marker, &format!("{injection}\n{marker}"), 1)
    )
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_settings_receipt_and_rollback_leave_no_untracked_analyst() {
    if isolated_child(
        "codex_voice::acp_regression_tests::failed_settings_receipt_and_rollback_leave_no_untracked_analyst",
    ) {
        return;
    }
    let source = slow_replacement_source();
    let (temp, home, sessions, mut settings, _) = fixture_with_source(&source).await;
    let previous = settings.clone();
    let receipt = home.daemon_dir().join(persistence::TEST_ANALYST_STATE_FILE);
    std::fs::remove_file(&receipt).expect("remove fixture receipt");
    std::fs::create_dir(&receipt).expect("make receipt unwritable");
    settings
        .command
        .extend(["--permission-mode".into(), "smart".into()]);
    let result = sessions
        .apply_analyst_settings(&home, settings, BTreeMap::new(), Vec::new(), false, false)
        .await;
    assert!(result.is_err(), "receipt write must fail");
    assert!(
        sessions.state.lock().await.analyst.is_none(),
        "successfully stopped rollback owner must be removed"
    );
    assert_eq!(
        cccc_core::codex_voice_settings::load(&home).expect("settings"),
        previous
    );
    let spawns =
        std::fs::read_to_string(temp.path().join("provider/fixture-spawns")).expect("spawn record");
    let pids = spawns
        .lines()
        .map(|line| line.parse::<u32>().expect("fixture pid"))
        .collect::<Vec<_>>();
    assert!(pids.len() >= 2, "candidate or rollback was not launched");
    assert!(
        pids.iter().all(|pid| !fixture_pid_alive(*pid)),
        "a failed replacement left a fixture process alive"
    );
    std::fs::remove_dir(&receipt).expect("restore receipt path");
    sessions.shutdown().await.expect("finish fixture cleanup");
}
