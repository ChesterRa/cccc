use super::*;
use crate::ops::codex_voice_analyst::{ActorLaunchConfig, AnalystEvent, AnalystSession};
use cccc_contracts::RuntimeMode;
use cccc_core::HomeLayout;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
use tokio::sync::broadcast;

const RUNTIMES: [ActorRuntime; 3] = [
    ActorRuntime::Kiro,
    ActorRuntime::Hermes,
    ActorRuntime::Droid,
];

const DIAGNOSTIC_PRIVATE_VALUE: &str = "opaque-private-runtime-value-cccc";
const DIAGNOSTIC_PROVIDER_TOKEN: &str = "sk-hermes-provider-private-1234567890";

fn reject_rpc(root: &Path, method: &str, details: Value) {
    std::fs::write(
        root.join("v3_rpc_rejection.json"),
        json!({
            "method":method,
            "error":{
                "code":-32603,
                "message":format!("Internal error: {DIAGNOSTIC_PRIVATE_VALUE}"),
                "data":{
                    "details":details,
                    "private_payload":"DO_NOT_PUBLISH_PROVIDER_DATA",
                    "nested":{"details":"DO_NOT_PUBLISH_NESTED_DIAGNOSTIC"}
                }
            }
        })
        .to_string(),
    )
    .expect("rejection fixture");
}

fn assert_safe_rpc_diagnostic(message: &str, method: &str) {
    assert!(message.contains(method), "missing failed method: {message}");
    assert!(message.contains("code -32603"));
    assert!(message.contains("Refresh request denied"));
    for private in [
        DIAGNOSTIC_PRIVATE_VALUE,
        DIAGNOSTIC_PROVIDER_TOKEN,
        "cccc-provider-bearer-credential",
        "cccc-provider-key-value",
        "multiline-private-first-line",
        "DO_NOT_PUBLISH_PROVIDER_DATA",
        "DO_NOT_PUBLISH_NESTED_DIAGNOSTIC",
    ] {
        assert!(
            !message.contains(private),
            "unsafe diagnostic contains {private}"
        );
    }
    assert!(
        message.chars().count() <= 1800,
        "unbounded startup diagnostic"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hermes_rpc_startup_diagnostics_keep_method_and_safe_details() {
    for method in [
        "initialize",
        "session/new",
        "session/load",
        "session/set_model",
    ] {
        let (temp, home, mut config) = fixture(ActorRuntime::Hermes);
        config.environment.insert(
            "CCCC_TEST_DIAGNOSTIC_PRIVATE".into(),
            DIAGNOSTIC_PRIVATE_VALUE.into(),
        );
        config.environment.insert(
            "CCCC_TEST_DIAGNOSTIC_MULTILINE".into(),
            "multiline-private-first-line\nmultiline-private-second-line".into(),
        );
        if method == "session/load" {
            let session = AnalystSession::launch_actor(&home, config.clone())
                .await
                .expect("first launch");
            materialize(&session).await;
            session.stop(session.generation()).await.expect("stop");
        }
        if method == "session/set_model" {
            config
                .command
                .extend(["--model".into(), "Fixture model".into()]);
        }
        reject_rpc(
            temp.path(),
            method,
            json!(format!(
                "Refresh request denied; multiline-private-first-line\nmultiline-private-second-line; {DIAGNOSTIC_PRIVATE_VALUE}; {DIAGNOSTIC_PROVIDER_TOKEN}; Authorization: Bearer cccc-provider-bearer-credential; api_key=cccc-provider-key-value {}",
                "界".repeat(1700),
            )),
        );
        let error = AnalystSession::launch_actor(&home, config)
            .await
            .err()
            .expect("rejected startup");
        assert_safe_rpc_diagnostic(&error.to_string(), method);
        assert_eq!(
            frames(temp.path())
                .iter()
                .filter(|frame| frame["method"] == "session/prompt")
                .count(),
            usize::from(method == "session/load")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hermes_rpc_prompt_diagnostics_are_safe_before_publication() {
    let (temp, home, mut config) = fixture(ActorRuntime::Hermes);
    config.environment.insert(
        "CCCC_TEST_DIAGNOSTIC_PRIVATE".into(),
        DIAGNOSTIC_PRIVATE_VALUE.into(),
    );
    let session = AnalystSession::launch_actor(&home, config)
        .await
        .expect("launch");
    reject_rpc(
        temp.path(),
        "session/prompt",
        json!(format!(
            "Refresh request denied because its service is busy; {DIAGNOSTIC_PROVIDER_TOKEN}; Authorization: Bearer cccc-provider-bearer-credential; api_key=cccc-provider-key-value",
        )),
    );
    let error = session
        .start_turn(session.generation(), "diagnostic", "fixture")
        .await
        .expect_err("prompt rejected");
    session.stop(session.generation()).await.expect("stop");
    assert_eq!(
        error.kind(),
        io::ErrorKind::Other,
        "explanatory busy is not prompt admission evidence"
    );
    assert_safe_rpc_diagnostic(&error.to_string(), "session/prompt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hermes_rpc_diagnostics_do_not_publish_non_scalar_details_or_other_runtime_data() {
    for (runtime, details) in [
        (
            ActorRuntime::Hermes,
            json!({"secret":"DO_NOT_PUBLISH_OBJECT_DETAILS"}),
        ),
        (
            ActorRuntime::Kiro,
            json!("DO_NOT_PUBLISH_OTHER_RUNTIME_DETAILS"),
        ),
    ] {
        let (temp, home, mut config) = fixture(runtime);
        config.environment.insert(
            "CCCC_TEST_DIAGNOSTIC_PRIVATE".into(),
            DIAGNOSTIC_PRIVATE_VALUE.into(),
        );
        reject_rpc(temp.path(), "session/new", details);
        let error = AnalystSession::launch_actor(&home, config)
            .await
            .err()
            .expect("rejected startup");
        let message = error.to_string();
        assert!(message.contains("session/new"));
        assert!(!message.contains(DIAGNOSTIC_PRIVATE_VALUE));
        assert!(!message.contains("DO_NOT_PUBLISH"));
    }
}

fn fixture(runtime: ActorRuntime) -> (tempfile::TempDir, HomeLayout, ActorLaunchConfig) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    home.initialize().expect("initialize");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("fixture", "")
        .expect("group");
    let program = temp.path().join(if runtime == ActorRuntime::Kiro {
        "kiro-cli"
    } else {
        name(runtime)
    });
    std::fs::write(&program, include_str!("v3_fixture.py")).expect("fixture program");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let environment = BTreeMap::from([
        (
            "HOME".into(),
            temp.path().join("provider").to_string_lossy().into_owned(),
        ),
        (
            "USERPROFILE".into(),
            temp.path().join("provider").to_string_lossy().into_owned(),
        ),
        (
            "KIRO_HOME".into(),
            temp.path()
                .join("provider/kiro")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "HERMES_HOME".into(),
            temp.path()
                .join("provider/hermes")
                .to_string_lossy()
                .into_owned(),
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
        ("ACP_FIXTURE_CLI".into(), "/bin/true".into()),
    ]);
    let config = ActorLaunchConfig {
        workdir: temp.path().into(),
        group_id: group.group_id,
        actor_id: "alpha".into(),
        runtime,
        runtime_mode: RuntimeMode::Acp,
        command: vec![program.to_string_lossy().into_owned()],
        environment,
    };
    (temp, home, config)
}

fn frames(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("v3_requests.jsonl"))
        .expect("frames")
        .lines()
        .map(|line| serde_json::from_str(line).expect("frame"))
        .collect()
}

async fn terminal(events: &mut broadcast::Receiver<AnalystEvent>) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.message["method"] == "turn/completed" {
                return event.message;
            }
        }
    })
    .await
    .expect("completion")
}

async fn materialize(session: &AnalystSession) {
    let mut events = session.subscribe();
    session
        .start_turn(session.generation(), "delivery", "fixture")
        .await
        .expect("prompt");
    assert_eq!(
        terminal(&mut events).await["params"]["turn"]["status"],
        "completed"
    );
}

/// Stop's cancel closes prompt admission in the same serial command: a
/// submission that reaches the loop after it is refused and never written,
/// even when stop found no turn to cancel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_stop_cancel_refuses_a_prompt_that_arrives_after_it() {
    let (temp, home, config) = fixture(ActorRuntime::Hermes);
    let session = AnalystSession::launch_actor(&home, config.clone())
        .await
        .expect("new");
    let crate::ops::codex_voice_analyst::ManagedProtocol::Acp(protocol) = &session.protocol else {
        panic!("Hermes uses ACP");
    };
    assert_eq!(
        protocol
            .cancel_and_settle(session.thread_id(), Duration::from_secs(2))
            .await,
        super::super::acp::CancelOutcome::NoTurn
    );
    let late = session
        .start_turn(session.generation(), "late", "LATE_PROMPT")
        .await
        .expect_err("a prompt after stop's cancel is refused");
    assert_eq!(late.kind(), io::ErrorKind::Interrupted);
    let receipt = super::super::super::runtime_session::native_acp::receipt_path(
        &home,
        &config.group_id,
        &config.actor_id,
        ActorRuntime::Hermes,
    )
    .expect("receipt path");
    assert_eq!(
        cccc_core::fs::read_json::<Value>(&receipt).expect("receipt")["attempted"],
        false,
        "a refused prompt leaves the empty session unattempted"
    );
    session.stop(session.generation()).await.expect("stop");
    assert!(
        !frames(temp.path())
            .iter()
            .any(|request| request.to_string().contains("LATE_PROMPT")),
        "the refused prompt never reached the provider"
    );
}

#[test]
fn v3_native_command_intent_preserves_permissions_and_owns_transport() {
    let kiro = options(ActorRuntime::Kiro, &[]).expect("default Kiro");
    assert_eq!(
        kiro.command,
        [
            "kiro-cli",
            "acp",
            "--agent-engine=v3",
            "--auth-method",
            "cli"
        ]
    );
    assert!(kiro.unrestricted);
    let hermes = options(ActorRuntime::Hermes, &[]).expect("default Hermes");
    assert_eq!(hermes.command, ["hermes", "acp"]);
    assert!(hermes.unrestricted);
    assert_eq!(
        hermes_viewer(&hermes, "session-1"),
        ["hermes", "--tui", "--yolo", "--resume", "session-1"]
    );
    // Both clients must open the session in its own Hermes profile.
    for command in [
        ["hermes", "-p", "work", "--tui"].as_slice(),
        ["hermes", "--profile", "work"].as_slice(),
        ["hermes", "--profile=work"].as_slice(),
    ] {
        let command = command
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect::<Vec<_>>();
        let profiled = options(ActorRuntime::Hermes, &command).expect("Hermes profile");
        assert_eq!(profiled.command, ["hermes", "--profile", "work", "acp"]);
        assert!(!profiled.unrestricted);
        assert_eq!(
            hermes_viewer(&profiled, "session-1"),
            [
                "hermes",
                "--profile",
                "work",
                "--tui",
                "--resume",
                "session-1"
            ]
        );
    }
    let droid = options(ActorRuntime::Droid, &[]).expect("default Droid");
    assert_eq!(droid.command, ["droid", "exec", "--output-format", "acp"]);
    assert_eq!(droid.mode.as_deref(), Some("auto-high"));
    assert!(
        !droid.unrestricted,
        "Droid's auto-high is a provider mode, not a host bypass"
    );
    for (runtime, program) in [
        (ActorRuntime::Kiro, "kiro-cli"),
        (ActorRuntime::Hermes, "hermes"),
        (ActorRuntime::Droid, "droid"),
    ] {
        assert!(
            !options(runtime, &[program.into()])
                .expect("interactive")
                .unrestricted
        );
        for flag in [
            "--resume",
            "--prompt",
            "--mcp-config",
            "--output-format",
            "--agent-engine",
            "--auth-method",
            "--trust-tools",
        ] {
            assert!(options(runtime, &[program.into(), flag.into(), "value".into()]).is_err());
        }
    }
    let unsafe_droid = options(
        ActorRuntime::Droid,
        &["droid".into(), "--skip-permissions-unsafe".into()],
    )
    .expect("explicit bypass");
    assert_eq!(
        unsafe_droid.command,
        [
            "droid",
            "exec",
            "--skip-permissions-unsafe",
            "--output-format",
            "acp"
        ]
    );
    assert!(unsafe_droid.unrestricted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_models_effort_modes_and_mcp_are_reapplied_to_owned_sessions() {
    for runtime in RUNTIMES {
        let (temp, home, mut config) = fixture(runtime);
        config
            .command
            .extend(["--model".into(), "Fixture model".into()]);
        match runtime {
            ActorRuntime::Kiro => config.command.extend([
                "--agent".into(),
                "fixture-agent".into(),
                "--effort".into(),
                "high".into(),
            ]),
            ActorRuntime::Droid => config.command.extend([
                "--auto".into(),
                "high".into(),
                "--reasoning-effort".into(),
                "medium".into(),
            ]),
            _ => {}
        }
        let first = AnalystSession::launch_actor(&home, config.clone())
            .await
            .expect("new");
        assert!(first.structured_only());
        let id = first.thread_id().to_owned();
        materialize(&first).await;
        first.stop(first.generation()).await.expect("stop");
        if runtime == ActorRuntime::Hermes {
            std::fs::write(temp.path().join("v3_list_paginated"), "").expect("pagination");
        }
        let loaded = AnalystSession::launch_actor(&home, config.clone())
            .await
            .expect("load");
        assert_eq!(loaded.thread_id(), id);
        assert!(loaded.thread_resumed);
        loaded.stop(loaded.generation()).await.expect("stop");
        let records = frames(temp.path());
        let (method, field) = if runtime == ActorRuntime::Hermes {
            ("session/set_model", "modelId")
        } else {
            ("session/set_config_option", "value")
        };
        let models: Vec<_> = records
            .iter()
            .filter(|r| {
                r["method"] == method
                    && (method == "session/set_model" || r["params"]["configId"] == "model")
            })
            .collect();
        assert_eq!(models.len(), 2);
        assert!(
            models
                .iter()
                .all(|r| r["params"][field] == format!("model-{}", name(runtime)))
        );
        if runtime != ActorRuntime::Hermes {
            assert_eq!(
                records
                    .iter()
                    .filter(|r| r["method"] == "session/set_mode")
                    .count(),
                2
            );
            let effort_id = if runtime == ActorRuntime::Kiro {
                "effortLevel"
            } else {
                "reasoning_effort"
            };
            assert_eq!(
                records
                    .iter()
                    .filter(|r| r["method"] == "session/set_config_option"
                        && r["params"]["configId"] == effort_id)
                    .count(),
                2
            );
        } else {
            assert_eq!(
                records
                    .iter()
                    .filter(|r| r["method"] == "session/list")
                    .count(),
                2
            );
        }
        for record in records
            .iter()
            .filter(|r| matches!(r["method"].as_str(), Some("session/new" | "session/load")))
        {
            let env = record["params"]["mcpServers"][0]["env"]
                .as_array()
                .expect("MCP env");
            assert!(
                env.iter()
                    .any(|entry| entry["name"] == "CCCC_ACTOR_ID" && entry["value"] == "alpha")
            );
            assert!(
                env.iter()
                    .any(|entry| entry["name"] == "CCCC_GROUP_ID"
                        && entry["value"] == config.group_id)
            );
        }
        config.command[2] = "unavailable".into();
        config.actor_id = "explicit-unavailable-model".into();
        let error = AnalystSession::launch_actor(&home, config)
            .await
            .err()
            .expect("explicit unavailable model rejected");
        assert!(error.to_string().contains("Unavailable selection"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_recovery_rejects_changed_identity_without_replacement() {
    for runtime in RUNTIMES {
        let (temp, home, config) = fixture(runtime);
        let first = AnalystSession::launch_actor(&home, config.clone())
            .await
            .expect("new");
        materialize(&first).await;
        first.stop(first.generation()).await.expect("stop");
        let receipt = super::super::super::runtime_session::native_acp::receipt_path(
            &home,
            &config.group_id,
            &config.actor_id,
            runtime,
        )
        .expect("path");
        let before = std::fs::read(&receipt).expect("receipt");
        std::fs::write(temp.path().join("v3_wrong_identity"), "").expect("wrong ID");
        let error = AnalystSession::launch_actor(&home, config)
            .await
            .err()
            .expect("changed identity rejected");
        assert!(
            error
                .to_string()
                .contains("changed the attempted session identity")
        );
        assert_eq!(std::fs::read(&receipt).expect("retained receipt"), before);
        assert_eq!(
            frames(temp.path())
                .iter()
                .filter(|r| r["method"] == "session/new")
                .count(),
            1
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_hermes_recovery_requires_exact_existing_workspace_session() {
    for marker in ["v3_list_unavailable", "v3_list_wrong_cwd", "v3_list_cycle"] {
        let (temp, home, config) = fixture(ActorRuntime::Hermes);
        let first = AnalystSession::launch_actor(&home, config.clone())
            .await
            .expect("new");
        materialize(&first).await;
        first.stop(first.generation()).await.expect("stop");
        std::fs::write(temp.path().join(marker), "").expect("list failure");
        assert!(AnalystSession::launch_actor(&home, config).await.is_err());
        let records = frames(temp.path());
        assert_eq!(
            records
                .iter()
                .filter(|r| r["method"] == "session/new")
                .count(),
            1
        );
        assert!(
            !records.iter().any(|r| r["method"] == "session/load"),
            "unverifiable load must not be attempted"
        );
        assert!(!records.iter().any(|r| r["method"] == "session/resume"));
    }
}

/// Earlier CCCC versions launched Hermes ACP with
/// `HERMES_ACP_SKIP_CONFIGURED_MCP=1`, so a conversation they saved carries it
/// in its launch fingerprint. The next start begins one new conversation and
/// notes why; the start after that resumes the new conversation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_hermes_conversation_saved_with_the_old_launch_flag_is_replaced_once() {
    use super::super::super::runtime_session::native_acp as receipts;
    const SAVED: &str = "v3-saved-by-an-earlier-cccc";
    let (temp, home, config) = fixture(ActorRuntime::Hermes);
    let receipt = receipts::receipt_path(
        &home,
        &config.group_id,
        &config.actor_id,
        ActorRuntime::Hermes,
    )
    .expect("receipt path");
    let mut earlier = config.environment.clone();
    earlier.insert("HERMES_ACP_SKIP_CONFIGURED_MCP".into(), "1".into());
    let (identity, parts) = receipts::launch(&config.workdir, &config.command, &earlier);
    std::fs::create_dir_all(receipt.parent().expect("receipt directory"))
        .expect("receipt directory");
    receipts::record(&receipt, &identity, &parts, SAVED, true).expect("earlier receipt");

    let first = AnalystSession::launch_actor(&home, config.clone())
        .await
        .expect("new conversation");
    materialize(&first).await;
    first.stop(first.generation()).await.expect("stop");
    let replaced: Value = cccc_core::fs::read_json(&receipt).expect("receipt");
    let current = replaced["provider_session_id"]
        .as_str()
        .expect("new conversation ID")
        .to_owned();
    assert_ne!(current, SAVED);
    assert_eq!(replaced["replaced"]["reasons"], json!(["launch_identity"]));
    assert_eq!(replaced["replaced"]["previous"]["provider_id"], SAVED);

    let second = AnalystSession::launch_actor(&home, config)
        .await
        .expect("resumed conversation");
    second.stop(second.generation()).await.expect("stop");
    let records = frames(temp.path());
    let calls = |method: &str| {
        records
            .iter()
            .filter(|r| r["method"] == method)
            .collect::<Vec<_>>()
    };
    assert_eq!(calls("session/new").len(), 1);
    assert_eq!(
        calls("session/load")
            .iter()
            .map(|r| &r["params"]["sessionId"])
            .collect::<Vec<_>>(),
        [&json!(current)]
    );
    let resumed: Value = cccc_core::fs::read_json(&receipt).expect("receipt");
    assert_eq!(resumed["provider_session_id"], current.as_str());
    assert_eq!(resumed["replaced"], replaced["replaced"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_unsupported_protocol_fails_before_creating_a_session() {
    for runtime in RUNTIMES {
        let (temp, home, config) = fixture(runtime);
        std::fs::write(temp.path().join("v3_bad_protocol"), "").expect("bad protocol");
        let error = AnalystSession::launch_actor(&home, config)
            .await
            .err()
            .expect("unsupported protocol");
        assert!(error.to_string().contains("protocol version 1"));
        assert!(
            !frames(temp.path())
                .iter()
                .any(|r| r["method"] == "session/new")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_interactive_permissions_remain_pending_and_cancel_is_not_completion() {
    for runtime in RUNTIMES {
        let (temp, home, config) = fixture(runtime);
        let session = AnalystSession::launch_actor(&home, config.clone())
            .await
            .expect("new");
        let mut events = session.subscribe();
        session
            .start_turn(session.generation(), "permission", "PERMISSION")
            .await
            .expect("prompt");
        tokio::time::timeout(Duration::from_secs(4), async {
            while session.permissions().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("approval");
        let request_id = session.permissions()[0]["request_id"]
            .as_str()
            .expect("request ID")
            .to_owned();
        let receipt_path = super::super::super::runtime_session::native_acp::receipt_path(
            &home,
            &config.group_id,
            &config.actor_id,
            runtime,
        )
        .expect("receipt path");
        let receipt = std::fs::read(&receipt_path).expect("attempted receipt");
        assert!(
            !frames(temp.path())
                .iter()
                .any(|r| r["id"] == "v3-approval" && r.get("result").is_some())
        );
        while let Ok(event) = events.try_recv() {
            assert_ne!(event.message["method"], "turn/completed");
        }
        session
            .interrupt(session.generation(), "ignored-by-acp")
            .await
            .expect("cancel");
        assert_eq!(
            terminal(&mut events).await["params"]["turn"]["status"],
            "cancelled"
        );
        assert!(session.permissions().is_empty());
        assert!(
            session
                .respond_permission(session.generation(), &request_id, true)
                .await
                .is_err(),
            "late approval has no authority after cancellation"
        );
        assert_eq!(
            std::fs::read(&receipt_path).expect("retained receipt"),
            receipt
        );
        // The same process takes the next prompt: a cancel retires neither the
        // process nor the conversation (Hermes included).
        materialize(&session).await;
        session.stop(session.generation()).await.expect("stop");
        assert_eq!(
            frames(temp.path())
                .iter()
                .filter(|r| r["method"] == "session/new")
                .count(),
            1,
            "cancellation does not create a replacement conversation"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v3_hermes_session_discovery_has_one_total_deadline() {
    let (temp, _home, config) = fixture(ActorRuntime::Hermes);
    std::fs::write(temp.path().join("v3_list_endless"), "").expect("endless pagination");
    let mut child = std::process::Command::new("python3")
        .arg(&config.command[0])
        .current_dir(temp.path())
        .env_clear()
        .envs(&config.environment)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("fixture process");
    let protocol = AcpClient::new(
        child.stdin.take().expect("stdin"),
        child.stdout.take().expect("stdout"),
        "fixture-generation".into(),
        "hermes",
        super::super::acp::PermissionPolicy::Interactive,
        super::super::acp::PromptCompletion::ResponseWithActivityReceipt,
    )
    .expect("protocol");
    let capabilities = protocol
        .request(
            "initialize",
            json!({"protocolVersion":1}),
            Duration::from_secs(4),
        )
        .await
        .expect("initialize");
    let error = verify_hermes_session(
        &protocol,
        &capabilities,
        temp.path(),
        "missing-id",
        Duration::from_millis(100),
    )
    .await
    .expect_err("continually new cursors cannot extend the deadline");
    protocol.close().await;
    let _ = child.kill();
    child.wait().expect("reap");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("not resumed"));
    let records = frames(temp.path());
    assert!(
        records
            .iter()
            .filter(|r| r["method"] == "session/list")
            .count()
            > 1
    );
    assert!(
        !records
            .iter()
            .any(|r| r["method"] == "session/load" || r["method"] == "session/new")
    );
}

/// The open prompt the control state reports goes with the loop: a loop
/// aborted while blocked on a write the provider never reads leaves none,
/// although the client itself is still alive.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_aborted_protocol_loop_reports_no_open_prompt() {
    let (temp, _home, config) = fixture(ActorRuntime::Hermes);
    // The provider answers session/new, then stops reading.
    std::fs::write(temp.path().join("v3_stall_stdin"), "").expect("stall marker");
    let mut child = std::process::Command::new("python3")
        .arg(&config.command[0])
        .current_dir(temp.path())
        .env_clear()
        .envs(&config.environment)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("fixture process");
    let protocol = std::sync::Arc::new(
        AcpClient::new(
            child.stdin.take().expect("stdin"),
            child.stdout.take().expect("stdout"),
            "fixture-generation".into(),
            "hermes",
            super::super::acp::PermissionPolicy::Interactive,
            super::super::acp::PromptCompletion::ResponseWithActivityReceipt,
        )
        .expect("protocol"),
    );
    protocol
        .request(
            "initialize",
            json!({"protocolVersion":1}),
            Duration::from_secs(4),
        )
        .await
        .expect("initialize");
    let session = protocol
        .request(
            "session/new",
            json!({"cwd":temp.path(),"mcpServers":[]}),
            Duration::from_secs(4),
        )
        .await
        .expect("session");
    let session_id = session["sessionId"]
        .as_str()
        .expect("session id")
        .to_owned();
    // The prompt's write completes into the pipe; nobody answers it.
    let prompt = {
        let protocol = std::sync::Arc::clone(&protocol);
        let session_id = session_id.clone();
        tokio::spawn(async move {
            protocol
                .start_prompt(&session_id, "open-prompt", "OPEN PROMPT")
                .await
        })
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !protocol.has_open_prompt() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the written prompt is open"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // A request larger than the pipe blocks the loop on its write.
    let blocked = {
        let protocol = std::sync::Arc::clone(&protocol);
        tokio::spawn(async move {
            protocol
                .request(
                    "fixture/pad",
                    json!({"pad":"x".repeat(1 << 20)}),
                    Duration::from_secs(30),
                )
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let open_while_blocked = protocol.has_open_prompt();
    let closed = protocol.close().await;
    let open_after_close = protocol.has_open_prompt();
    // Reap before asserting: a failure would otherwise leave the blocked write
    // waiting on a provider that never reads, and the test would hang.
    let _ = child.kill();
    child.wait().expect("reap");
    prompt.abort();
    blocked.abort();
    assert!(open_while_blocked, "still open while the loop is blocked");
    assert!(!closed, "the blocked loop is aborted");
    assert!(!open_after_close, "an ended loop holds no open prompt");
}
