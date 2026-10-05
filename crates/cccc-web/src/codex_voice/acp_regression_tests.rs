//! Offline provider protocol and isolated launch state; no real calls or login.
use super::*;
use cccc_contracts::{ActorRuntime, CodexVoiceAnalystSettings, RuntimeMode};
use serde_json::{Value, json};
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, time::Duration};

fn isolated_child(name: &str) -> bool {
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

async fn fixture() -> (
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
    std::fs::write(
        &program,
        include_str!(
            "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
        ),
    )
    .expect("protocol fixture");
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
