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
        .env("CCCC_HOME", temp.path().join("cccc-home"))
        .env("CODEX_HOME", temp.path().join("codex"))
        .env("GROK_HOME", temp.path().join("grok"))
        .env_remove("GROK_AUTH_PATH")
        .env_remove("CCCC_CODEX_AUTH_PATH")
        .output()
        .expect("fixture child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success()
            && stdout
                .lines()
                .any(|line| line.starts_with("test result: ok. 1 passed;")),
        "isolated test {name} failed or did not run:\n{stdout}\n{stderr}"
    );
    true
}

#[test]
fn isolated_child_rejects_a_missing_test() {
    let error = std::panic::catch_unwind(|| isolated_child("a_test_that_does_not_exist"))
        .expect_err("an empty child run must not pass");
    let message = error.downcast_ref::<String>().expect("failure message");
    assert!(message.contains("failed or did not run"));
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
    let (temp, home, sessions, settings, env) = cold_fixture_with_source(source);
    let runtime = Arc::new(
        persistence::launch_analyst(&home)
            .await
            .expect("warm Analyst"),
    );
    runtime.start_monitor(
        home.clone(),
        Arc::clone(&sessions.chet_history_write_failed),
    );
    persistence::persist_analyst(&home, &runtime, false).expect("empty receipt");
    sessions.state.lock().await.analyst = Some(runtime);
    (temp, home, sessions, settings, env)
}

fn cold_fixture_with_source(
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
    let sessions = CodexVoiceSessions::default();
    (temp, home, sessions, settings, env)
}

struct ChetHttpFixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    router: axum::Router,
    state: crate::AppState,
    token: String,
    client: cccc_client::DaemonClient,
    daemon: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl ChetHttpFixture {
    async fn new() -> Self {
        let source = include_str!(
            "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
        );
        let (temp, home, _, _, _) = cold_fixture_with_source(source);
        let daemon_home = home.clone();
        let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
        let client =
            cccc_client::DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(2));
        tokio::time::timeout(Duration::from_secs(10), async {
            while !client
                .call(&chet_daemon_request("ping", json!({})))
                .await
                .is_ok_and(|r| r.ok)
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("fixture daemon ready");
        let tokens = cccc_core::access_tokens::AccessTokenStore::new(home.clone()).expect("tokens");
        let token = tokens
            .create("admin", vec![], true, None)
            .expect("admin")
            .token;
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (router, _, _, state, _) = crate::app_with_shutdown_and_recovery(
            home.clone(),
            shutdown,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "fixture".into(),
            None,
        );
        Self {
            _temp: temp,
            home,
            router,
            state,
            token,
            client,
            daemon,
        }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Value,
    ) -> (axum::http::StatusCode, Value) {
        use tower::ServiceExt;
        let request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {}", self.token),
            )
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .expect("request");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).expect("JSON response"),
        )
    }

    async fn wait_for(&self, id: uuid::Uuid, kind: &str) -> Vec<Value> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (status, value) = self
                    .request("GET", &format!("/api/v1/chet/inputs/{id}"), json!(null))
                    .await;
                assert_eq!(status, axum::http::StatusCode::OK);
                let records = value["result"]["records"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if records.iter().any(|record| record["kind"] == kind) {
                    break records;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("expected history fact")
    }

    async fn stop(self) {
        self.state
            .codex_voice
            .shutdown()
            .await
            .expect("stop fixture Analyst");
        self.client
            .call(&chet_daemon_request("shutdown", json!({})))
            .await
            .expect("daemon shutdown");
        tokio::time::timeout(Duration::from_secs(10), self.daemon)
            .await
            .expect("daemon stops")
            .expect("daemon task")
            .expect("daemon result");
    }
}

fn chet_daemon_request(op: &str, args: Value) -> cccc_contracts::DaemonRequest {
    cccc_contracts::DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("object").clone(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_http_submits_once_and_exposes_history_without_a_voice_call() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_http_submits_once_and_exposes_history_without_a_voice_call",
    ) {
        return;
    }
    let fixture = ChetHttpFixture::new().await;
    let (status, empty) = fixture
        .request("GET", "/api/v1/chet/history", json!(null))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(empty["result"]["records"], json!([]));
    assert!(!fixture.home.root().join("state/chet").exists());

    let id = uuid::Uuid::new_v4();
    let body = json!({"id":id,"request":{"text":"HTTP fixture request","context":null}});
    let (status, first) = fixture
        .request("POST", "/api/v1/chet/inputs", body.clone())
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(first["result"]["registration"], "inserted");
    let (status, duplicate) = fixture.request("POST", "/api/v1/chet/inputs", body).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(duplicate["result"]["registration"], "existing");
    let records = fixture.wait_for(id, "result").await;
    assert_eq!(records.iter().filter(|r| r["kind"] == "input").count(), 1);
    assert_eq!(records.iter().filter(|r| r["kind"] == "receipt").count(), 1);
    assert!(records.iter().any(|r| {
        r["kind"] == "result"
            && r["outcome"]["text"]
                .as_str()
                .is_some_and(|s| s.contains("HTTP fixture request"))
    }));
    let requests = cccc_core::codex_voice_settings::workdir(&fixture.home)
        .expect("workdir")
        .join("fixture_requests.jsonl");
    let prompt_count = std::fs::read_to_string(requests)
        .expect("fixture requests")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|request| request["method"] == "session/prompt")
        .count();
    assert_eq!(prompt_count, 1);
    assert!(fixture.state.codex_voice.current().await.call.is_none());
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_http_reports_a_known_pre_admission_failure() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_http_reports_a_known_pre_admission_failure",
    ) {
        return;
    }
    let fixture = ChetHttpFixture::new().await;
    std::fs::remove_file(fixture._temp.path().join("devin")).expect("remove fake Runtime");
    let id = uuid::Uuid::new_v4();
    let body = json!({"id":id,"request":{"text":"not submitted","context":null}});
    let (status, first) = fixture
        .request("POST", "/api/v1/chet/inputs", body.clone())
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(first["result"]["registration"], "inserted");
    let records = fixture.wait_for(id, "not_started").await;
    assert!(
        records
            .iter()
            .any(|r| r["kind"] == "not_started" && r["reason"] == "assistant_unavailable")
    );
    assert!(
        !records
            .iter()
            .any(|r| matches!(r["kind"].as_str(), Some("receipt" | "result")))
    );
    let (_, retry) = fixture.request("POST", "/api/v1/chet/inputs", body).await;
    assert_eq!(retry["result"]["registration"], "existing");
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_text_reuses_an_existing_assistant_voice_owner() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_text_reuses_an_existing_assistant_voice_owner",
    ) {
        return;
    }
    let fixture = ChetHttpFixture::new().await;
    let runtime = Arc::new(
        persistence::launch_analyst(&fixture.home)
            .await
            .expect("fake Analyst"),
    );
    runtime.start_monitor(
        fixture.home.clone(),
        Arc::clone(&fixture.state.codex_voice.chet_history_write_failed),
    );
    persistence::persist_analyst(&fixture.home, &runtime, false).expect("persist fixture owner");
    fixture.state.codex_voice.state.lock().await.analyst = Some(Arc::clone(&runtime));
    let call =
        local_assistant_call(&fixture.home, &fixture.state.codex_voice, &runtime, false).await;
    let id = uuid::Uuid::new_v4();
    let body = json!({"id":id,"request":{"text":"text while Voice is active","context":null}});
    assert_eq!(
        fixture.request("POST", "/api/v1/chet/inputs", body).await.0,
        axum::http::StatusCode::OK
    );
    let records = fixture.wait_for(id, "result").await;
    assert!(records.iter().any(|record| {
        record["kind"] == "result"
            && record["outcome"]["text"]
                .as_str()
                .is_some_and(|text| text.contains("text while Voice is active"))
    }));
    let state = fixture.state.codex_voice.state.lock().await;
    assert!(
        state
            .active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, &call))
    );
    assert!(
        state
            .analyst
            .as_ref()
            .is_some_and(|analyst| Arc::ptr_eq(analyst, &runtime))
    );
    drop(state);
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_cancel_records_runtime_outcome_and_unsent_queue_separately() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_cancel_records_runtime_outcome_and_unsent_queue_separately",
    ) {
        return;
    }
    let fixture = ChetHttpFixture::new().await;
    let active = uuid::Uuid::new_v4();
    let queued = uuid::Uuid::new_v4();
    let first = json!({"id":active,"request":{"text":"HOLD active Chet task","context":null}});
    assert_eq!(
        fixture
            .request("POST", "/api/v1/chet/inputs", first)
            .await
            .0,
        axum::http::StatusCode::OK
    );
    fixture.wait_for(active, "receipt").await;
    let second = json!({"id":queued,"request":{"text":"queued Chet task","context":null}});
    assert_eq!(
        fixture
            .request("POST", "/api/v1/chet/inputs", second)
            .await
            .0,
        axum::http::StatusCode::OK
    );
    let generation = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (_, status) = fixture
                .request("GET", "/api/v1/chet/status", json!(null))
                .await;
            if status["result"]["queued"]
                .as_array()
                .is_some_and(|ids| ids.contains(&json!(queued)))
            {
                break status["result"]["generation"]
                    .as_str()
                    .expect("generation")
                    .to_owned();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("input queued");
    let (status, accepted) = fixture
        .request(
            "POST",
            &format!("/api/v1/codex_voice/analysts/{generation}/cancel"),
            json!(null),
        )
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(accepted["result"]["cancelled"], true);
    let first_facts = fixture.wait_for(active, "result").await;
    assert!(
        first_facts
            .iter()
            .any(|record| record["kind"] == "result" && record["outcome"]["status"] == "cancelled")
    );
    let second_facts = fixture.wait_for(queued, "not_started").await;
    assert!(
        second_facts
            .iter()
            .any(|record| record["kind"] == "not_started" && record["reason"] == "cancelled")
    );
    assert!(
        !second_facts
            .iter()
            .any(|record| matches!(record["kind"].as_str(), Some("receipt" | "result")))
    );
    let requests = cccc_core::codex_voice_settings::workdir(&fixture.home)
        .expect("workdir")
        .join("fixture_requests.jsonl");
    let prompts = std::fs::read_to_string(requests).expect("fixture requests");
    assert!(!prompts.contains("queued Chet task"));
    fixture.stop().await;
}

#[tokio::test]
async fn chet_status_does_not_wait_for_an_analyst_launch() {
    let sessions = CodexVoiceSessions::new();
    let id = uuid::Uuid::new_v4();
    let _preparing = sessions.track_chet_preparation(id);
    let lock = sessions.state.lock().await;
    let status = tokio::time::timeout(Duration::from_millis(100), sessions.chet_status())
        .await
        .expect("status must not wait behind the launching owner");
    assert_eq!(status["owner_busy"], true);
    assert_eq!(status["preparing"], json!([id]));
    drop(lock);
    assert_eq!(sessions.chet_status().await["owner_busy"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_group_owner_recovers_unclaimed_inputs_without_replaying_claims() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_group_owner_recovers_unclaimed_inputs_without_replaying_claims",
    ) {
        return;
    }
    let fixture = ChetHttpFixture::new().await;
    let group = cccc_core::GroupStore::new(fixture.home.clone())
        .expect("groups")
        .create("group input fixture", "")
        .expect("group");
    let mut sources = Vec::new();
    for text in [
        "first group input",
        "claimed group input",
        "third group input",
    ] {
        let sent = fixture
            .client
            .call(&chet_daemon_request(
                "send",
                json!({
                    "by":"user","group_id":group.group_id,"to":["@chet"],
                    "message_mode":"send","text":text
                }),
            ))
            .await
            .expect("send");
        assert!(sent.ok, "fixture event was not accepted");
        sources.push(
            sent.result["event"]["id"]
                .as_str()
                .expect("event ID")
                .to_owned(),
        );
    }
    let mut registered = Vec::new();
    for event_id in &sources[..2] {
        let input = cccc_contracts::chet::GroupInput {
            group_id: group.group_id.clone(),
            group_generation: cccc_core::connect_groups::generation(&group),
            event_id: event_id.clone(),
            sender: cccc_contracts::chet::GroupInputSender::User,
            reply_to: None,
        };
        let result = fixture
            .client
            .call(&chet_daemon_request(
                "chet_history_group_input",
                json!({"by":"user","input":input}),
            ))
            .await
            .expect("register");
        assert!(result.ok, "fixture registration was rejected");
        registered
            .push(uuid::Uuid::parse_str(result.result["id"].as_str().expect("id")).expect("UUID"));
    }
    let claim = fixture
        .client
        .call(&chet_daemon_request(
            "chet_history_claim_group_input",
            json!({"by":"user","input_id":registered[1]}),
        ))
        .await
        .expect("claim");
    assert_eq!(claim.result["registration"], "inserted");

    // The public history page and per-input reconciliation return the same
    // bounded source view, without a separate request for each Group message.
    let (status, history) = fixture
        .request("GET", "/api/v1/chet/history", json!(null))
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let first_id = registered[0].to_string();
    assert_eq!(
        history["result"]["sources"][&first_id]["status"],
        "available"
    );
    assert_eq!(
        history["result"]["sources"][&first_id]["text"],
        "first group input"
    );
    let (status, detail) = fixture
        .request(
            "GET",
            &format!("/api/v1/chet/inputs/{first_id}"),
            json!(null),
        )
        .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        detail["result"]["sources"][&first_id],
        history["result"]["sources"][&first_id]
    );

    // The first input was durably registered but not attempted; the second
    // could already have reached the model; the third is only in the ledger.
    crate::chet_group_input::spawn(fixture.state.clone());
    fixture.wait_for(registered[0], "result").await;
    let third = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let history = fixture
                .client
                .call(&chet_daemon_request(
                    "chet_history_list",
                    json!({"by":"user","after":"0","limit":100}),
                ))
                .await
                .expect("history");
            if let Some(id) = history.result["records"]
                .as_array()
                .expect("records")
                .iter()
                .find(|row| row["kind"] == "group_input" && row["input"]["event_id"] == sources[2])
                .and_then(|row| row["id"].as_str())
            {
                break uuid::Uuid::parse_str(id).expect("UUID");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("third input registered");
    fixture.wait_for(third, "result").await;
    let claimed = fixture
        .client
        .call(&chet_daemon_request(
            "chet_history_find",
            json!({"by":"user","id":registered[1]}),
        ))
        .await
        .expect("claimed history");
    assert!(
        claimed.result["records"]
            .as_array()
            .expect("records")
            .iter()
            .any(|row| row["kind"] == "group_dispatch")
    );
    assert!(
        !claimed.result["records"]
            .as_array()
            .expect("records")
            .iter()
            .any(|row| row["kind"] == "receipt" || row["kind"] == "result")
    );

    let requests = cccc_core::codex_voice_settings::workdir(&fixture.home)
        .expect("workdir")
        .join("fixture_requests.jsonl");
    let seen: Vec<String> = std::fs::read_to_string(requests)
        .expect("fixture requests")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|request| request["method"] == "session/prompt")
        .filter_map(|request| {
            request["params"]["prompt"][0]["text"]
                .as_str()
                .and_then(|text| text.lines().last())
                .and_then(|line| serde_json::from_str::<Value>(line).ok())
                .and_then(|body| body["event_id"].as_str().map(str::to_owned))
        })
        .collect();
    assert_eq!(seen, vec![sources[0].clone(), sources[2].clone()]);
    let _ = fixture.state.shutdown.send(());
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_pending_group_sources_reconcile_after_restart() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_pending_group_sources_reconcile_after_restart",
    ) {
        return;
    }
    let fixture = ChetHttpFixture::new().await;
    let groups = cccc_core::GroupStore::new(fixture.home.clone()).expect("groups");
    let mut invalid = Vec::new();
    let mut unreadable = Vec::new();
    let mut claimed = None;

    for case in [
        "removed",
        "replaced",
        "new_group",
        "deleted",
        "missing_event",
        "unreadable",
        "transient_missing",
        "removal_in_progress",
        "replacement_in_progress",
        "claimed_removed",
    ] {
        let mut group = groups.create(case, "").expect("group");
        let mut worker = cccc_contracts::Actor::new("worker");
        worker.generation = "original-member".into();
        worker.runtime = cccc_contracts::ActorRuntime::WebModel;
        group.actors.push(worker.clone());
        groups.save(&group).expect("member");
        let original_group = group.clone();
        let mut added = cccc_contracts::Event::new("actor.add", &group.group_id);
        added.data.insert("actor".into(), json!(worker));
        let ledger = groups.ledger_path(&group.group_id).expect("ledger");
        cccc_core::ledger::append(&ledger, &added).expect("member registration");
        let sent = fixture
            .client
            .call(&chet_daemon_request(
                "send",
                json!({"group_id":group.group_id,"by":"worker","to":["@chet"],
                    "message_mode":"send","text":"Please help"}),
            ))
            .await
            .expect("send");
        assert!(sent.ok, "{sent:?}");
        let input = cccc_contracts::chet::GroupInput {
            group_id: group.group_id.clone(),
            group_generation: cccc_core::connect_groups::generation(&group),
            event_id: sent.result["event"]["id"].as_str().expect("id").into(),
            sender: cccc_contracts::chet::GroupInputSender::Actor {
                actor_id: "worker".into(),
                actor_generation: "original-member".into(),
            },
            reply_to: None,
        };
        let registered = fixture
            .client
            .call(&chet_daemon_request(
                "chet_history_group_input",
                json!({"by":"user","input":input}),
            ))
            .await
            .expect("register");
        assert!(registered.ok, "{registered:?}");
        let id =
            uuid::Uuid::parse_str(registered.result["id"].as_str().expect("id")).expect("UUID");
        match case {
            "removed" => {
                group.actors.clear();
                groups.save(&group).expect("remove member");
                let mut event = cccc_contracts::Event::new("actor.remove", &group.group_id);
                event.data.insert("actor_id".into(), json!("worker"));
                cccc_core::ledger::append(&ledger, &event).expect("commit removal");
            }
            "replaced" => {
                group.actors[0].generation = "replacement-member".into();
                groups.save(&group).expect("replace member");
                let mut event = cccc_contracts::Event::new("actor.add", &group.group_id);
                event.data.insert("actor".into(), json!(group.actors[0]));
                cccc_core::ledger::append(&ledger, &event).expect("commit replacement");
            }
            "new_group" => {
                group.generation = "replacement-group".into();
                groups.save(&group).expect("replace group");
            }
            "deleted" => assert!(groups.delete(&group.group_id).expect("delete group")),
            "missing_event" => std::fs::write(&ledger, b"").expect("replace fixture ledger"),
            "unreadable" => {
                let path = ledger.parent().expect("group directory").join("group.yaml");
                std::fs::write(&path, "[invalid group document").expect("corrupt fixture");
                unreadable.push((case, id, input.clone(), "io_error", None));
            }
            "transient_missing" => {
                let dir = ledger.parent().expect("group directory");
                let tombstone = dir.with_file_name(format!(".{}.deleting-test", group.group_id));
                std::fs::rename(dir, tombstone).expect("simulate deletion before commit");
                unreadable.push((case, id, input.clone(), "io_error", None));
            }
            "removal_in_progress" => {
                group.actors.clear();
                groups.save(&group).expect("uncommitted removal");
                unreadable.push((
                    case,
                    id,
                    input.clone(),
                    "invalid_chet_source",
                    Some(original_group.clone()),
                ));
            }
            "replacement_in_progress" => {
                group.actors[0].generation = "replacement-member".into();
                groups.save(&group).expect("uncommitted replacement");
                unreadable.push((
                    case,
                    id,
                    input.clone(),
                    "invalid_chet_source",
                    Some(original_group.clone()),
                ));
            }
            "claimed_removed" => {
                let result = fixture
                    .client
                    .call(&chet_daemon_request(
                        "chet_history_claim_group_input",
                        json!({"by":"user","input_id":id}),
                    ))
                    .await
                    .expect("claim");
                assert_eq!(result.result["registration"], "inserted");
                group.actors.clear();
                groups.save(&group).expect("remove member after claim");
                claimed = Some(id);
            }
            _ => unreachable!(),
        }
        if ![
            "unreadable",
            "transient_missing",
            "removal_in_progress",
            "replacement_in_progress",
            "claimed_removed",
        ]
        .contains(&case)
        {
            invalid.push((case, id));
        }
    }

    crate::chet_group_input::spawn(fixture.state.clone());
    for (case, id) in invalid {
        let records =
            tokio::time::timeout(Duration::from_secs(2), fixture.wait_for(id, "not_started"))
                .await
                .unwrap_or_else(|_| panic!("{case} must be surfaced as not started"));
        let fact = records
            .iter()
            .find(|record| {
                record["kind"] == "not_started" && record["reason"] == "source_unavailable"
            })
            .unwrap_or_else(|| panic!("{case}: {records:?}"));
        assert!(
            !records.iter().any(|record| {
                ["group_dispatch", "receipt", "result"]
                    .contains(&record["kind"].as_str().unwrap_or(""))
            }),
            "{case} must never reach the model"
        );
        let retried = fixture
            .client
            .call(&chet_daemon_request(
                "chet_history_not_started",
                json!({"by":"user","id":fact["id"],
                "input_id":id,"reason":"source_unavailable"}),
            ))
            .await
            .expect("idempotent retry");
        assert_eq!(retried.result["registration"], "existing", "{case}");
    }
    // The owner cannot register a later feed event until it has finished the
    // initial pending reconciliation. This makes the negative assertions
    // below independent of HashMap iteration order and scheduler timing.
    let barrier = groups.create("reconcile barrier", "").expect("group");
    let sent = fixture
        .client
        .call(&chet_daemon_request(
            "send",
            json!({"group_id":barrier.group_id,"by":"user","to":["@chet"],
                "message_mode":"send","text":"barrier"}),
        ))
        .await
        .expect("barrier event");
    assert!(sent.ok, "{sent:?}");
    let barrier_id = sent.result["event"]["id"].as_str().expect("id");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = fixture
                .client
                .call(&chet_daemon_request(
                    "chet_history_list",
                    json!({"by":"user","after":"0","limit":100}),
                ))
                .await
                .expect("history");
            if page.result["records"]
                .as_array()
                .expect("records")
                .iter()
                .any(|record| {
                    record["kind"] == "group_input" && record["input"]["event_id"] == barrier_id
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owner finished reconciliation");
    for (case, id, input, expected_error, rollback) in unreadable {
        let response = fixture
            .client
            .call(&chet_daemon_request(
                "chet_history_group_input",
                json!({"by":"user","input":input}),
            ))
            .await
            .expect("validation response");
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error").code,
            expected_error,
            "{case} is not confirmed source retirement"
        );
        let (_, detail) = fixture
            .request("GET", &format!("/api/v1/chet/inputs/{id}"), json!(null))
            .await;
        assert!(
            !detail["result"]["records"]
                .as_array()
                .expect("facts")
                .iter()
                .any(|record| record["kind"] == "not_started")
        );
        if let Some(original) = rollback {
            groups.save(&original).expect("roll back member mutation");
            let response = fixture
                .client
                .call(&chet_daemon_request(
                    "chet_history_group_input",
                    json!({"by":"user","input":input}),
                ))
                .await
                .expect("validation after rollback");
            assert!(response.ok, "{case}: {response:?}");
            assert_eq!(response.result["registration"], "existing");
            assert_eq!(response.result["id"], id.to_string());
        }
    }
    let id = claimed.expect("claimed input");
    let (_, detail) = fixture
        .request("GET", &format!("/api/v1/chet/inputs/{id}"), json!(null))
        .await;
    let facts = detail["result"]["records"].as_array().expect("facts");
    assert!(
        facts
            .iter()
            .any(|record| record["kind"] == "group_dispatch")
    );
    assert!(
        !facts.iter().any(|record| record["kind"] == "not_started"),
        "a claimed input remains unconfirmed"
    );
    let _ = fixture.state.shutdown.send(());
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_group_prompt_resolves_only_a_visible_reply_source() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_group_prompt_resolves_only_a_visible_reply_source",
    ) {
        return;
    }
    let fixture = ChetHttpFixture::new().await;
    let groups = cccc_core::GroupStore::new(fixture.home.clone()).expect("groups");
    let mut group = groups.create("source context fixture", "").expect("group");
    let mut worker = cccc_contracts::Actor::new("worker");
    worker.generation = "worker-generation".into();
    worker.runtime = cccc_contracts::ActorRuntime::WebModel;
    let mut other = cccc_contracts::Actor::new("other");
    other.runtime = cccc_contracts::ActorRuntime::WebModel;
    group.actors.extend([worker.clone(), other]);
    groups.save(&group).expect("actors");
    let mut registration = cccc_contracts::Event::new("actor.add", &group.group_id);
    registration.data.insert("actor".into(), json!(worker));
    cccc_core::ledger::append(
        &groups.ledger_path(&group.group_id).expect("ledger"),
        &registration,
    )
    .expect("actor registration");
    let token = "1".repeat(32);
    cccc_core::voice_notifications::register_origin(
        &fixture.home,
        &token,
        "fixture",
        "thread",
        cccc_contracts::ActorRuntime::Codex,
    )
    .expect("Chet origin");
    let client = &fixture.client;
    let send = |op: &'static str, args: Value| async move {
        client.call(&chet_daemon_request(op, args)).await
    };
    let handoff = send(
        "send",
        json!({"group_id":group.group_id,"by":"user",
        "to":["worker"],"message_mode":"send","text":"Old visible handoff",
        "_voice_origin":token}),
    )
    .await
    .expect("handoff");
    assert!(handoff.ok, "{handoff:?}");
    let handoff_id = handoff.result["event"]["id"].as_str().expect("id");
    for index in 0..31 {
        let sent = send(
            "send",
            json!({"group_id":group.group_id,"by":"user",
            "to":["worker"],"message_mode":"send","text":format!("Later context {index}")}),
        )
        .await
        .expect("context");
        assert!(sent.ok, "{sent:?}");
    }
    let visible_reply = send(
        "reply",
        json!({"group_id":group.group_id,"by":"worker",
        "reply_to":handoff_id,"text":"Finished that handoff"}),
    )
    .await
    .expect("reply");
    assert!(visible_reply.ok, "{visible_reply:?}");
    let private = send(
        "send",
        json!({"group_id":group.group_id,"by":"user","to":["other"],
        "message_mode":"send","text":"A private message to someone else"}),
    )
    .await
    .expect("private");
    assert!(private.ok, "{private:?}");
    let private_id = private.result["event"]["id"].as_str().expect("private id");
    let hidden_reply = send(
        "send",
        json!({"group_id":group.group_id,"by":"worker",
        "to":["@chet"],"message_mode":"send","reply_to":private_id,"text":"Question"}),
    )
    .await
    .expect("hidden reply");
    assert!(hidden_reply.ok, "{hidden_reply:?}");

    let mut registered = Vec::new();
    for (response, parent) in [(&visible_reply, handoff_id), (&hidden_reply, private_id)] {
        let input = cccc_contracts::chet::GroupInput {
            group_id: group.group_id.clone(),
            group_generation: cccc_core::connect_groups::generation(&group),
            event_id: response.result["event"]["id"].as_str().expect("id").into(),
            sender: cccc_contracts::chet::GroupInputSender::Actor {
                actor_id: "worker".into(),
                actor_generation: "worker-generation".into(),
            },
            reply_to: Some(parent.into()),
        };
        let result = send(
            "chet_history_group_input",
            json!({"by":"user","input":input}),
        )
        .await
        .expect("register");
        assert!(result.ok, "{result:?}");
        registered
            .push(uuid::Uuid::parse_str(result.result["id"].as_str().expect("id")).expect("UUID"));
    }
    crate::chet_group_input::spawn(fixture.state.clone());
    for id in registered {
        fixture.wait_for(id, "result").await;
    }
    let requests = cccc_core::codex_voice_settings::workdir(&fixture.home)
        .expect("workdir")
        .join("fixture_requests.jsonl");
    let prompts: Vec<Value> = std::fs::read_to_string(requests)
        .expect("fixture requests")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|request| request["method"] == "session/prompt")
        .filter_map(|request| {
            request["params"]["prompt"][0]["text"]
                .as_str()
                .and_then(|text| text.lines().last())
                .and_then(|line| serde_json::from_str::<Value>(line).ok())
        })
        .collect();
    let visible = prompts
        .iter()
        .find(|body| body["event_id"] == visible_reply.result["event"]["id"])
        .expect("visible reply prompt");
    assert_eq!(visible["replied_message"]["status"], "available");
    assert_eq!(
        visible["replied_message"]["event"]["data"]["text"],
        "Old visible handoff"
    );
    let hidden = prompts
        .iter()
        .find(|body| body["event_id"] == hidden_reply.result["event"]["id"])
        .expect("hidden reply prompt");
    assert_eq!(hidden["replied_message"]["status"], "unavailable");
    assert!(
        !hidden
            .to_string()
            .contains("A private message to someone else")
    );
    let _ = fixture.state.shutdown.send(());
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_group_result_reuses_voice_output_without_a_second_model_turn() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_group_result_reuses_voice_output_without_a_second_model_turn",
    ) {
        return;
    }
    use cccc_core::{voice_notifications as voice, voice_recording_lease as lease};
    for (already_viewed, live_call) in [(false, false), (true, false), (false, true)] {
        let fixture = ChetHttpFixture::new().await;
        let groups = cccc_core::GroupStore::new(fixture.home.clone()).expect("groups");
        let mut group = groups.create("Voice handoff fixture", "").expect("group");
        let mut worker = cccc_contracts::Actor::new("worker");
        worker.generation = "worker-generation".into();
        worker.runtime = cccc_contracts::ActorRuntime::WebModel;
        group.actors.push(worker.clone());
        groups.save(&group).expect("save");
        let mut added = cccc_contracts::Event::new("actor.add", &group.group_id);
        added.data.insert("actor".into(), json!(worker));
        cccc_core::ledger::append(&groups.ledger_path(&group.group_id).expect("path"), &added)
            .expect("registration");
        let session = if live_call {
            fixture
                .state
                .codex_voice
                .prepare_chet_for_group_input(&fixture.home)
                .await
                .expect("fake Analyst");
            let runtime = fixture
                .state
                .codex_voice
                .state
                .lock()
                .await
                .analyst
                .clone()
                .expect("Analyst");
            Some(
                local_assistant_call(&fixture.home, &fixture.state.codex_voice, &runtime, true)
                    .await,
            )
        } else {
            None
        };
        let token = "6".repeat(32);
        voice::register_origin(
            &fixture.home,
            &token,
            "fixture",
            "thread",
            cccc_contracts::ActorRuntime::Codex,
        )
        .expect("origin");
        let acquired = if live_call {
            None
        } else {
            Some(
                lease::update(
                    &fixture.home,
                    "",
                    "",
                    &json!({"action":"acquire",
                "owner_id":"codex-voice:fixture","capture_mode":"codex_voice",
                "dispatch_target":"codex_voice","by":"user"}),
                )
                .expect("lease"),
            )
        };
        let parent = fixture
            .client
            .call(&chet_daemon_request(
                "send",
                json!({"by":"user",
            "group_id":group.group_id,"to":["worker"],"message_mode":"send",
            "text":"Voice handoff","_voice_origin":token}),
            ))
            .await
            .expect("parent");
        assert!(parent.ok, "{parent:?}");
        if let Some(acquired) = acquired {
            assert!(
                lease::release(
                    &fixture.home,
                    "",
                    "codex-voice:fixture",
                    acquired["lease_id"].as_str().expect("lease id")
                )
                .expect("release")
            );
        }
        if let Some(session) = &session {
            session.start_notifications(
                fixture.home.clone(),
                fixture.state.ledger_events.clone(),
                crate::auth::Principal {
                    user_id: "local".into(),
                    allowed_groups: Vec::new(),
                    is_admin: true,
                    raw_token: String::new(),
                },
            );
        }
        let reply = fixture
            .client
            .call(&chet_daemon_request(
                "reply",
                json!({"by":"worker",
            "group_id":group.group_id,"reply_to":parent.result["event"]["id"],
            "text":"Actor reply after call"}),
            ))
            .await
            .expect("reply");
        assert!(reply.ok, "{reply:?}");
        let source = cccc_contracts::voice_notifications::VoiceMessageRef {
            group_id: group.group_id.clone(),
            event_id: reply.result["event"]["id"].as_str().expect("id").into(),
        };
        if already_viewed {
            voice::mark_viewed(&fixture.home, std::slice::from_ref(&source)).expect("viewed");
        }
        voice::scan(&fixture.home).expect("Voice scan before Group owner");
        assert!(
            voice::reserve(&fixture.home, &source, "other-owner")
                .expect("reserve")
                .is_none()
        );
        if live_call {
            // Give the old consumer a polling interval to encounter the
            // candidate before the Group owner submits it.
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let candidate = voice::snapshot(&fixture.home)
                .expect("state")
                .messages
                .into_iter()
                .find(|item| item.source == source)
                .expect("candidate");
            assert!(
                candidate.handoff.is_none(),
                "Voice must leave Group input to its owner"
            );
        }
        let input = cccc_contracts::chet::GroupInput {
            group_id: group.group_id.clone(),
            group_generation: cccc_core::connect_groups::generation(&group),
            event_id: source.event_id.clone(),
            sender: cccc_contracts::chet::GroupInputSender::Actor {
                actor_id: "worker".into(),
                actor_generation: "worker-generation".into(),
            },
            reply_to: parent.result["event"]["id"].as_str().map(str::to_owned),
        };
        let registered = fixture
            .client
            .call(&chet_daemon_request(
                "chet_history_group_input",
                json!({"by":"user","input":input}),
            ))
            .await
            .expect("register");
        assert!(registered.ok, "{registered:?}");
        let id =
            uuid::Uuid::parse_str(registered.result["id"].as_str().expect("id")).expect("UUID");
        crate::chet_group_input::spawn(fixture.state.clone());
        fixture.wait_for(id, "result").await;
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(result) = voice::snapshot(&fixture.home)
                    .expect("Voice state")
                    .results
                    .into_iter()
                    .next()
                {
                    break result;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("Voice result must be retained");
        assert_eq!(result.sources, vec![source.clone()]);
        let reserved = voice::reserve_output(&fixture.home, &result.id, "next-call")
            .expect("reserve output")
            .expect("result");
        let output =
            voice::prepare_output(&fixture.home, &reserved.id, "next-call").expect("prepare");
        assert_eq!(
            output.is_none(),
            already_viewed,
            "the original viewed policy remains in effect"
        );
        let requests = cccc_core::codex_voice_settings::workdir(&fixture.home)
            .expect("workdir")
            .join("fixture_requests.jsonl");
        let count = std::fs::read_to_string(requests)
            .expect("fixture requests")
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|request| request["method"] == "session/prompt")
            .filter(|request| {
                request["params"]["prompt"][0]["text"]
                    .as_str()
                    .is_some_and(|text| text.contains(&source.event_id))
            })
            .count();
        assert_eq!(count, 1, "the fixture observed one model submission");
        let _ = fixture.state.shutdown.send(());
        fixture.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_group_voice_binding_failure_stops_the_current_notification_consumer() {
    if isolated_child(
        "codex_voice::acp_regression_tests::a_group_voice_binding_failure_stops_the_current_notification_consumer",
    ) {
        return;
    }
    use cccc_contracts::voice_notifications::NotificationScope;
    use cccc_core::voice_notifications as voice;

    let fixture = ChetHttpFixture::new().await;
    fixture
        .state
        .codex_voice
        .prepare_chet_for_group_input(&fixture.home)
        .await
        .expect("fake Analyst");
    let runtime = fixture
        .state
        .codex_voice
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("Analyst");
    let session =
        local_assistant_call(&fixture.home, &fixture.state.codex_voice, &runtime, true).await;
    let groups = cccc_core::GroupStore::new(fixture.home.clone()).expect("groups");
    let group = groups.create("Notification fixture", "").expect("group");
    let mut preferences = voice::preferences(&fixture.home).expect("preferences");
    preferences
        .groups
        .insert(group.group_id.clone(), NotificationScope::ToUser);
    voice::save_preferences(&fixture.home, preferences).expect("subscribe");
    let mut other = cccc_contracts::Event::new("chat.message", &group.group_id);
    other.by = "worker".into();
    other.data = json!({"text":"Ordinary notification", "to":["user"]})
        .as_object()
        .expect("event")
        .clone();
    cccc_core::ledger::append(&groups.ledger_path(&group.group_id).expect("path"), &other)
        .expect("append");
    voice::scan(&fixture.home).expect("ordinary candidate");
    let state_path = fixture
        .home
        .root()
        .join("state/codex_voice/notifications.json");
    let saved = std::fs::read(&state_path).expect("fixture state");
    std::fs::write(&state_path, "{").expect("inject binding failure");
    let input = cccc_contracts::chet::GroupInput {
        group_id: group.group_id.clone(),
        group_generation: cccc_core::connect_groups::generation(&group),
        event_id: "synthetic-source".into(),
        sender: cccc_contracts::chet::GroupInputSender::Actor {
            actor_id: "worker".into(),
            actor_generation: "generation".into(),
        },
        reply_to: Some("synthetic-parent".into()),
    };
    let request = cccc_core::chet_history::TextRequest {
        text: "fixture input".into(),
        context: None,
    };
    fixture
        .state
        .codex_voice
        .submit_chet_group_input(&fixture.home, uuid::Uuid::new_v4(), &input, &request)
        .await
        .expect("Group input is still admitted");
    assert!(
        *session.notification_paused.borrow(),
        "binding failure is visible"
    );
    std::fs::write(&state_path, saved).expect("restore fixture state");
    session.start_notifications(
        fixture.home.clone(),
        fixture.state.ledger_events.clone(),
        crate::auth::Principal {
            user_id: "local".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            raw_token: String::new(),
        },
    );
    tokio::time::sleep(Duration::from_millis(1300)).await;
    let candidate = voice::snapshot(&fixture.home)
        .expect("snapshot")
        .messages
        .into_iter()
        .find(|item| item.source.event_id == other.id)
        .expect("ordinary notification retained");
    assert!(
        candidate.handoff.is_none(),
        "a paused call must not consume later candidates"
    );
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_history_write_does_not_hold_up_voice_analyst_events() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_history_write_does_not_hold_up_voice_analyst_events",
    ) {
        return;
    }
    let (_temp, home, sessions, _, _) = fixture().await;
    let socket_path = home.daemon_dir().join("chet-test.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("fake daemon socket");
    let address = json!({"v":1,"transport":"unix","path":socket_path,
        "host":"","port":0,"pid":std::process::id(),"version":"test","ts":"test"});
    std::fs::write(
        home.daemon_dir().join("ccccd.addr.json"),
        address.to_string(),
    )
    .expect("fake daemon address");
    let (accepted, wait_accepted) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("history writer connects");
        let _ = accepted.send(());
        std::future::pending::<()>().await;
    });
    let id = uuid::Uuid::new_v4();
    let request = cccc_core::chet_history::TextRequest {
        text: "nonblocking history fixture".into(),
        context: None,
    };
    sessions
        .submit_chet_input(&home, id, &request)
        .await
        .expect("submit");
    tokio::time::timeout(Duration::from_secs(2), wait_accepted)
        .await
        .expect("history writer reached daemon")
        .expect("accept event");
    let analyst = sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("Analyst");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !analyst
            .info()
            .last_result
            .contains("nonblocking history fixture")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Voice projection must not wait for stalled history IPC");
    let status = sessions.chet_status().await;
    assert!(
        status["working"]
            .as_array()
            .expect("working IDs")
            .contains(&json!(id)),
        "a completed turn remains visible while its history write is blocked: {status}"
    );
    server.abort();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = sessions.chet_status().await;
            if status["history_write_failed"] == true
                && !status["working"]
                    .as_array()
                    .expect("working IDs")
                    .contains(&json!(id))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("failed write must be visible before the input leaves working");
    sessions.shutdown().await.expect("stop fixture Analyst");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_voice_socket_records_observed_transcript_with_acknowledgement() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_voice_socket_records_observed_transcript_with_acknowledgement",
    ) {
        return;
    }
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

    let fixture = ChetHttpFixture::new().await;
    let runtime = Arc::new(
        persistence::launch_analyst(&fixture.home)
            .await
            .expect("fake Analyst"),
    );
    runtime.start_monitor(
        fixture.home.clone(),
        Arc::clone(&fixture.state.codex_voice.chet_history_write_failed),
    );
    persistence::persist_analyst(&fixture.home, &runtime, false).expect("persist fixture owner");
    fixture.state.codex_voice.state.lock().await.analyst = Some(Arc::clone(&runtime));
    let session =
        local_assistant_call(&fixture.home, &fixture.state.codex_voice, &runtime, false).await;
    let generation = session.call.generation().to_owned();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Web listener");
    let address = listener.local_addr().expect("address");
    let web_router = fixture.router.clone();
    let web = tokio::spawn(async move {
        axum::serve(listener, web_router).await.expect("serve");
    });
    let mut request = format!("ws://{address}/api/v1/codex_voice/calls/{generation}/events")
        .into_client_request()
        .expect("socket request");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", fixture.token).parse().expect("header"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("Voice socket");
    let id = uuid::Uuid::new_v4();
    let message = json!({"type":"transcript","id":id,"turn_id":"local-1","seq":1,
        "role":"user","text":"an observed transcript","final_update":true,"complete":true});
    for (value, expected) in [
        (message.clone(), "saved"),
        (message.clone(), "saved"),
        (
            json!({"type":"transcript","id":id,"turn_id":"local-1","seq":1,
                 "role":"user","text":"different","final_update":true,"complete":true}),
            "not_saved",
        ),
    ] {
        socket
            .send(Message::Text(value.to_string().into()))
            .await
            .expect("send transcript");
        let receipt = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Message::Text(text) = socket.next().await.expect("frame").expect("socket")
                else {
                    continue;
                };
                let value: Value = serde_json::from_str(&text).expect("JSON");
                if value["type"] == "transcript_receipt" && value["id"] == id.to_string() {
                    break value;
                }
            }
        })
        .await
        .expect("receipt");
        assert_eq!(receipt["result"], expected);
    }
    let (_, history) = fixture
        .request("GET", "/api/v1/chet/history", json!(null))
        .await;
    let records = history["result"]["records"].as_array().expect("records");
    assert_eq!(
        records
            .iter()
            .filter(|record| record["kind"] == "transcript")
            .count(),
        1
    );
    assert!(
        records
            .iter()
            .any(|record| record["transcript"]["call_id"] == generation
                && record["transcript"]["text"] == "an observed transcript"
                && record["transcript"]["complete"] == true)
    );
    let delegation = json!({"type":"provider_event","event":{"type":"delegation.created","item":{
        "type":"delegation","target":"client","id":"voice-fixture-request",
        "content":[{"type":"input_text","text":"inspect the spoken request"}]}}});
    socket
        .send(Message::Text(delegation.to_string().into()))
        .await
        .expect("send fake Voice delegation");
    let mut last_facts = Vec::new();
    let facts = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (_, history) = fixture
                .request("GET", "/api/v1/chet/history", json!(null))
                .await;
            let records = history["result"]["records"]
                .as_array()
                .cloned()
                .expect("records");
            last_facts = records
                .iter()
                .map(|record| {
                    json!({"kind":record["kind"],"id":record["id"],
                "input_id":record["input_id"],"input_ids":record["input_ids"]})
                })
                .collect();
            let observed = records
                .iter()
                .find(|record| record["kind"] == "voice_delegation");
            if let Some(observed) = observed {
                let id = &observed["id"];
                let receipt = records
                    .iter()
                    .any(|record| record["kind"] == "receipt" && &record["input_id"] == id);
                let result = records.iter().any(|record| {
                    record["kind"] == "result"
                        && record["input_ids"]
                            .as_array()
                            .is_some_and(|ids| ids.contains(id))
                });
                if receipt && result {
                    break records;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    let facts = match facts {
        Ok(facts) => facts,
        Err(_) => {
            let mut messages = Vec::new();
            while let Ok(Some(Ok(Message::Text(text)))) =
                tokio::time::timeout(Duration::from_millis(50), socket.next()).await
            {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    messages.push(json!({"type":value["type"],"code":value["code"]}));
                }
            }
            let info = runtime.info();
            panic!(
                "Voice facts did not converge: {last_facts:?}; socket={messages:?}; phase={} warning={}",
                info.phase, info.warning
            );
        }
    };
    assert!(
        facts
            .iter()
            .any(|record| record["kind"] == "voice_delegation"
                && record["delegation"]["call_id"] == generation
                && record["delegation"]["text"] == "inspect the spoken request")
    );
    assert!(facts.iter().any(|record| {
        record["kind"] == "result"
            && record["outcome"]["text"]
                .as_str()
                .is_some_and(|text| text.contains("inspect the spoken request"))
    }));
    let hold = json!({"type":"provider_event","event":{"type":"delegation.created","item":{
        "type":"delegation","target":"client","id":"voice-fixture-held",
        "content":[{"type":"input_text","text":"HOLD voice status fixture"}]}}});
    socket
        .send(Message::Text(hold.to_string().into()))
        .await
        .expect("send held Voice task");
    let (held_id, analyst_generation) = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (_, history) = fixture
                .request("GET", "/api/v1/chet/history", json!(null))
                .await;
            let records = history["result"]["records"].as_array().expect("records");
            let observed = records.iter().find(|record| {
                record["kind"] == "voice_delegation"
                    && record["delegation"]["delegation_id"] == "voice-fixture-held"
            });
            if let Some(observed) = observed {
                let id = &observed["id"];
                let has_receipt = records
                    .iter()
                    .any(|record| record["kind"] == "receipt" && &record["input_id"] == id);
                let (_, status) = fixture
                    .request("GET", "/api/v1/chet/status", json!(null))
                    .await;
                if has_receipt
                    && status["result"]["working"]
                        .as_array()
                        .is_some_and(|ids| ids.contains(id))
                {
                    break (
                        id.as_str().expect("voice id").to_owned(),
                        status["result"]["generation"]
                            .as_str()
                            .expect("Analyst generation")
                            .to_owned(),
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("held Voice task is visible as working");
    assert_eq!(
        fixture
            .request(
                "POST",
                &format!("/api/v1/codex_voice/analysts/{analyst_generation}/cancel"),
                json!(null)
            )
            .await
            .0,
        axum::http::StatusCode::OK
    );
    let held_facts = fixture
        .wait_for(uuid::Uuid::parse_str(&held_id).expect("voice id"), "result")
        .await;
    assert!(
        held_facts
            .iter()
            .any(|record| record["kind"] == "result" && record["outcome"]["status"] == "cancelled")
    );
    socket.close(None).await.expect("close socket");
    tokio::time::timeout(Duration::from_secs(5), async {
        while fixture
            .state
            .codex_voice
            .state
            .lock()
            .await
            .active
            .is_some()
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("assistant call closed");

    let context = cccc_contracts::codex_voice::VoiceApplicationContext::new_with_mode(
        "persona-fixture".into(),
        "local only".into(),
        cccc_contracts::codex_voice::VoiceCallMode::Persona,
    )
    .expect("persona context");
    let call = Arc::new(
        CodexVoiceCall::start(&fixture.home, None, Some(context))
            .await
            .expect("persona lease"),
    );
    let persona_generation = call.generation().to_owned();
    fixture.state.codex_voice.state.lock().await.active = Some(Arc::new(ActiveSession {
        provider_connection: None,
        verbosity: Default::default(),
        notification_paused: tokio::sync::watch::channel(false).0,
        call,
        analyst: None,
        client_session_id: "persona".into(),
        offer_digest: [0; 32],
        answer_sdp: String::new(),
        voice: "cove".into(),
        connection_state: AtomicU8::new(CONNECTION_UNATTACHED),
    }));
    let mut request =
        format!("ws://{address}/api/v1/codex_voice/calls/{persona_generation}/events")
            .into_client_request()
            .expect("persona request");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", fixture.token).parse().expect("header"),
    );
    let (mut persona, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("persona socket");
    let persona_id = uuid::Uuid::new_v4();
    let persona_text = json!({"type":"transcript","id":persona_id,"turn_id":"persona-1","seq":1,
        "role":"user","text":"persona stays separate","final_update":true,"complete":true});
    persona
        .send(Message::Text(persona_text.to_string().into()))
        .await
        .expect("persona transcript");
    let receipt = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Message::Text(text) = persona.next().await.expect("frame").expect("socket") else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).expect("JSON");
            if value["type"] == "transcript_receipt" && value["id"] == persona_id.to_string() {
                break value;
            }
        }
    })
    .await
    .expect("persona receipt");
    assert_eq!(receipt["result"], "not_saved");
    let (_, history) = fixture
        .request("GET", "/api/v1/chet/history", json!(null))
        .await;
    assert!(
        !history["result"]["records"]
            .as_array()
            .expect("records")
            .iter()
            .any(|r| r["id"] == persona_id.to_string())
    );
    persona.close(None).await.expect("close persona");
    web.abort();
    fixture.stop().await;
}

#[tokio::test(flavor = "current_thread")]
async fn text_preparation_starts_without_realtime_and_voice_reuses_its_owner() {
    if isolated_child(
        "codex_voice::acp_regression_tests::text_preparation_starts_without_realtime_and_voice_reuses_its_owner",
    ) {
        return;
    }
    let source = include_str!(
        "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
    );
    let (temp, home, sessions, _, _) = cold_fixture_with_source(source);
    let state = Arc::clone(&sessions.state).lock_owned().await;
    let (state, analyst) = sessions
        .prepare_analyst_locked(&home, state)
        .await
        .expect("prepare text Analyst without Realtime credentials");
    assert!(
        state.active.is_none(),
        "text preparation must not open a call"
    );
    assert!(
        state
            .analyst
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(owner, &analyst)),
        "the manager must retain the text Analyst"
    );
    drop(state);

    // A missing local credential stops the synthetic call after owner selection;
    // no provider is contacted. The owner chosen for Voice must be the one
    // created for text, even when opening the recording/call fails afterwards.
    let config = RealtimeCallConfig {
        provider: cccc_contracts::codex_voice::VoiceProvider::Codex,
        grok_executable: None,
        auth_path: temp.path().join("missing-auth.json"),
        base_url: "http://127.0.0.1:1".into(),
        voice: "cove".into(),
        preferences: Default::default(),
        application_context: None,
    };
    let error = match sessions
        .start_with_realtime(&home, "text-first", "offer", config)
        .await
    {
        Ok(_) => panic!("the fixture deliberately has no Realtime credentials"),
        Err(error) => error,
    };
    assert!(
        matches!(
            error.downcast_ref::<super::start_error::StartStage>(),
            Some(super::start_error::StartStage::Realtime)
        ),
        "Voice must pass Analyst selection before the fixture fails: {error:#}"
    );
    let state = sessions.state.lock().await;
    assert!(state.active.is_none());
    assert!(
        state
            .analyst
            .as_ref()
            .is_some_and(|owner| Arc::ptr_eq(owner, &analyst)),
        "a later Voice start must reuse the text Analyst"
    );
    drop(state);
    sessions.shutdown().await.expect("stop fixture owner");
}

#[tokio::test(flavor = "current_thread")]
async fn text_input_reaches_the_cold_analyst_without_opening_voice() {
    if isolated_child(
        "codex_voice::acp_regression_tests::text_input_reaches_the_cold_analyst_without_opening_voice",
    ) {
        return;
    }
    let source = include_str!(
        "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
    );
    let (_temp, home, sessions, _, _) = cold_fixture_with_source(source);
    let id = uuid::Uuid::new_v4();
    let request = cccc_core::chet_history::TextRequest {
        text: "inspect the fixture".into(),
        context: None,
    };
    sessions
        .submit_chet_input(&home, id, &request)
        .await
        .expect("submit text to isolated Analyst");
    let requests = cccc_core::codex_voice_settings::workdir(&home)
        .expect("workdir")
        .join("fixture_requests.jsonl");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let found = std::fs::read_to_string(&requests)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|message| {
                    message["method"] == "session/prompt"
                        && message["params"]["prompt"][0]["text"]
                            .as_str()
                            .is_some_and(|text| text.contains("inspect the fixture"))
                });
            if found {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the fake Runtime received the input");
    assert!(sessions.state.lock().await.active.is_none());
    sessions.shutdown().await.expect("stop fixture owner");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chet_input_records_runtime_receipt_and_result_through_daemon() {
    if isolated_child(
        "codex_voice::acp_regression_tests::chet_input_records_runtime_receipt_and_result_through_daemon",
    ) {
        return;
    }
    let source = include_str!(
        "../../../cccc-daemon/src/ops/codex_voice_analyst/tests/native_acp_fixture.py"
    );
    let (_temp, home, sessions, _, _) = cold_fixture_with_source(source);
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(2));
    let request = |op: &str, args: Value| cccc_contracts::DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("fixture object").clone(),
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        while !client
            .call(&request("ping", json!({})))
            .await
            .is_ok_and(|reply| reply.ok)
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture daemon ready");
    let id = uuid::Uuid::new_v4();
    let input = cccc_core::chet_history::TextRequest {
        text: "inspect the fixture".into(),
        context: None,
    };
    let register = request(
        "chet_history_register",
        json!({"by":"user","id":id,"request":input}),
    );
    let registered = client.call(&register).await.expect("register");
    assert_eq!(registered.result["registration"], "inserted");
    sessions
        .submit_chet_input(&home, id, &input)
        .await
        .expect("submit");
    let records = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let reply = client
                .call(&request("chet_history_find", json!({"by":"user","id":id})))
                .await
                .expect("history");
            let records = reply.result["records"].as_array().expect("records").clone();
            if records.iter().any(|record| record["kind"] == "result") {
                break records;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("recorded result");
    assert_eq!(
        records
            .iter()
            .filter(|record| record["kind"] == "input")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record["kind"] == "receipt")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record["kind"] == "result")
            .count(),
        1
    );
    assert!(records.iter().any(|record| {
        record["kind"] == "result"
            && record["outcome"]["text"]
                .as_str()
                .is_some_and(|text| text.contains("inspect the fixture"))
    }));
    assert_eq!(
        client.call(&register).await.expect("retry").result["registration"],
        "existing"
    );
    assert!(sessions.state.lock().await.active.is_none());
    sessions.shutdown().await.expect("stop Analyst");
    client
        .call(&request("shutdown", json!({})))
        .await
        .expect("stop fixture daemon");
    tokio::time::timeout(Duration::from_secs(10), daemon)
        .await
        .expect("daemon stopped")
        .expect("daemon task")
        .expect("daemon");
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
    for (id, text, expected_status) in [
        ("rejected", "REJECT_BEFORE_RECEIPT", "unconfirmed"),
        ("failure", "FAIL_AFTER_RECEIPT", "failed"),
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
        assert_eq!(task.status, expected_status);
        assert!(!task.error.is_empty());
        let outputs = runtime.manual_results_for_call(second.call.generation());
        let output = outputs
            .iter()
            .find(|(key, _)| key == &task.id)
            .expect("failure returned");
        if expected_status == "unconfirmed" {
            assert!(
                output.1["content"][0]["text"]
                    .as_str()
                    .expect("text")
                    .contains("do not assert whether the work occurred")
            );
        }
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

#[tokio::test]
async fn unknown_manual_outcome_is_not_described_as_failure_to_voice() {
    if isolated_child(
        "codex_voice::acp_regression_tests::unknown_manual_outcome_is_not_described_as_failure_to_voice",
    ) {
        return;
    }
    use cccc_daemon::experimental_codex_voice::AnalystCompletionSource;
    let (_temp, _home, sessions, _, _) = fixture().await;
    let runtime = sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("Analyst");
    let id = "manual-input:unknown";
    runtime
        .register_manual_task(id, "fixture question", Some("call-a"))
        .await
        .expect("task");
    runtime.project_manual_event(&AnalystLifecycleEvent::Completed {
        source: AnalystCompletionSource::HostUnconfirmed,
        turn_id: "unknown-turn".into(),
        delegation_id: id.into(),
        delegation_ids: vec![id.into()],
        status: "failed".into(),
        error: "unconfirmed".into(),
        result: String::new(),
        speakable: false,
    });
    let output = runtime.manual_results_for_call("call-a");
    let text = output[0].1["content"][0]["text"].as_str().expect("text");
    let data: Value = serde_json::from_str(text.split_once('\n').expect("data").1).expect("JSON");
    assert_eq!(data["status"], "unconfirmed");
    assert!(text.contains("do not assert whether the work occurred"));
    sessions.shutdown().await.expect("cleanup");
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

#[tokio::test(flavor = "current_thread")]
async fn web_shutdown_waits_for_the_active_call_and_analyst() {
    if isolated_child(
        "codex_voice::acp_regression_tests::web_shutdown_waits_for_the_active_call_and_analyst",
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
def slow_stop(signum, frame):
    Path(os.environ["HOME"]).joinpath("fixture-term").write_text("term")
    time.sleep(6)
signal.signal(signal.SIGTERM, slow_stop)
"#;
    let source = format!(
        "{}\nimport time\nwhile True:\n    time.sleep(1)\n",
        source.replacen(marker, &format!("{injection}\n{marker}"), 1)
    );
    let (temp, home, original_sessions, _, _) = fixture_with_source(&source).await;
    let analyst = original_sessions
        .state
        .lock()
        .await
        .analyst
        .clone()
        .expect("fixture Analyst");
    let (shutdown, _) = tokio::sync::broadcast::channel(1);
    let (_router, _workers, _browsers, state, voice_shutdown) =
        crate::app_with_shutdown_and_recovery(
            home.clone(),
            shutdown.clone(),
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "voice-shutdown-fixture".into(),
            None,
        );
    state.codex_voice.state.lock().await.analyst = Some(Arc::clone(&analyst));
    let _active = local_assistant_call(&home, &state.codex_voice, &analyst, true).await;
    shutdown.send(()).expect("shutdown listeners");
    crate::await_codex_voice_shutdown(voice_shutdown, Duration::from_secs(4)).await;
    assert!(temp.path().join("provider/fixture-term").exists());
    let state = state.codex_voice.state.lock().await;
    assert!(state.active.is_none(), "recording lease must be released");
    assert!(state.analyst.is_none(), "Analyst shutdown must finish");
    drop(state);
    original_sessions.state.lock().await.analyst = None;
}
