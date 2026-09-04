use super::super::{
    AcpClient, AnalystSession, ManagedProtocol, SessionPurpose, WorkspaceBinding,
    acp::{PermissionPolicy, PromptCompletion},
    grok, process,
};
use crate::ops::codex_voice_lifecycle::{
    AnalystLifecycle, AnalystLifecycleEvent, VoiceDelegationAdmission,
};
use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const FAKE_SESSION_ID: &str = "01a0623c-19b3-7ec3-b777-95e24279ec67";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grok_adapter_keeps_one_session_across_acp_tui_and_resume() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
    home.initialize().expect("initialize");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let executable = fake_grok(temp.path());
    let command = vec![
        executable.to_string_lossy().into_owned(),
        "--model".into(),
        "grok-test".into(),
    ];
    let environment = BTreeMap::new();

    let first = launch(
        &home,
        &workspace,
        &command,
        &environment,
        None,
        "generation-1",
    )
    .await
    .expect("fresh Grok session");
    assert_eq!(first.session_id, FAKE_SESSION_ID);
    assert!(!first.resumed);
    assert!(
        first
            .tui_command
            .windows(2)
            .any(|parts| parts == ["--resume", FAKE_SESSION_ID])
    );

    let mut events = first.protocol.subscribe();
    let turn_id = first
        .protocol
        .start_prompt(FAKE_SESSION_ID, "delegation-1", "probe")
        .await
        .expect("accepted prompt");
    let started = next_method(&mut events, "turn/started").await;
    assert_eq!(
        started.requested_delegation_id.as_deref(),
        Some("delegation-1")
    );
    assert_eq!(started.message["params"]["turn"]["id"], turn_id);
    let delta = next_method(&mut events, "item/agentMessage/delta").await;
    assert_eq!(delta.message["params"]["delta"], "fake result");
    let completed = next_method(&mut events, "turn/completed").await;
    assert_eq!(completed.message["params"]["turn"]["id"], turn_id);
    assert_eq!(completed.message["params"]["turn"]["status"], "completed");
    stop(first).await;

    let resumed = launch(
        &home,
        &workspace,
        &command,
        &environment,
        Some(FAKE_SESSION_ID),
        "generation-2",
    )
    .await
    .expect("resumed Grok session");
    assert_eq!(resumed.session_id, FAKE_SESSION_ID);
    assert!(resumed.resumed);
    stop(resumed).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_tui_activity_blocks_delivery_until_it_is_idle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
    home.initialize().expect("initialize");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let executable = fake_grok(temp.path());
    let launched = launch(
        &home,
        &workspace,
        &[executable.to_string_lossy().into_owned()],
        &BTreeMap::new(),
        None,
        "generation-tui",
    )
    .await
    .expect("managed session");
    let analyst = analyst_from_grok(launched, &workspace, "generation-tui");
    let mut events = analyst.subscribe();
    let lifecycle = match &analyst.protocol {
        ManagedProtocol::Acp(protocol) => protocol.lifecycle_control(),
        ManagedProtocol::Codex(_) | ManagedProtocol::Claude(_) => {
            panic!("expected ACP protocol")
        }
    };

    lifecycle
        .status(FAKE_SESSION_ID, true)
        .await
        .expect("external busy");
    let started = next_method(&mut events, "turn/started").await;
    let external_turn = started.message["params"]["turn"]["id"]
        .as_str()
        .expect("external turn id")
        .to_owned();
    assert!(external_turn.starts_with("acp-tui-"));
    assert!(started.requested_delegation_id.is_none());

    let error = analyst
        .start_turn("generation-tui", "queued-delivery", "must wait")
        .await
        .expect_err("delivery must not overlap native TUI work");
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);

    lifecycle
        .status(FAKE_SESSION_ID, false)
        .await
        .expect("external idle");
    let completed = next_method(&mut events, "turn/completed").await;
    assert_eq!(completed.message["params"]["turn"]["id"], external_turn);

    let turn_id = analyst
        .start_turn("generation-tui", "queued-delivery", "must wait")
        .await
        .expect("same delivery accepted after TUI settles")
        .turn_id;
    assert_eq!(
        next_method(&mut events, "turn/started").await.message["params"]["turn"]["id"],
        turn_id
    );
    assert_eq!(
        next_method(&mut events, "turn/completed").await.message["params"]["turn"]["id"],
        turn_id
    );
    analyst.stop("generation-tui").await.expect("stop analyst");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn busy_runtime_accepts_voice_input_for_its_native_steer_or_queue_policy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
    home.initialize().expect("initialize");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let executable = fake_grok(temp.path());
    let launched = launch(
        &home,
        &workspace,
        &[executable.to_string_lossy().into_owned()],
        &BTreeMap::new(),
        None,
        "generation-voice-busy",
    )
    .await
    .expect("managed session");
    let session = Arc::new(analyst_from_grok(
        launched,
        &workspace,
        "generation-voice-busy",
    ));
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    let control = match &session.protocol {
        ManagedProtocol::Acp(protocol) => protocol.lifecycle_control(),
        ManagedProtocol::Codex(_) | ManagedProtocol::Claude(_) => {
            panic!("expected ACP protocol")
        }
    };

    control
        .status(FAKE_SESSION_ID, true)
        .await
        .expect("external busy");
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("started event timeout")
            .expect("started event"),
        AnalystLifecycleEvent::Started {
            origin: crate::ops::codex_voice_lifecycle::AnalystTurnOrigin::Terminal,
            ..
        }
    ));

    let admission = tokio::time::timeout(
        Duration::from_millis(250),
        lifecycle.admit_voice("voice-during-terminal", "do not hide this input"),
    )
    .await
    .expect("busy delegation admission must not wait")
    .expect("native input registration");
    assert_eq!(
        admission,
        VoiceDelegationAdmission::NativeInput {
            delegation_id: "voice-during-terminal".into(),
            text: "do not hide this input".into(),
        }
    );
    assert!(
        control
            .user_text(FAKE_SESSION_ID, "do not hide this input")
            .await
            .expect("authoritative native input echo")
    );
    assert!(matches!(
        events.recv().await.expect("association event"),
        AnalystLifecycleEvent::Associated {
            origin: crate::ops::codex_voice_lifecycle::AnalystTurnOrigin::Voice,
            ..
        }
    ));

    control
        .status(FAKE_SESSION_ID, false)
        .await
        .expect("external idle");
    session
        .stop(session.generation())
        .await
        .expect("stop session");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_busy_before_prompt_admission_remains_retryable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
    home.initialize().expect("initialize");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let executable = fake_grok_busy_once(temp.path());
    let launched = launch(
        &home,
        &workspace,
        &[executable.to_string_lossy().into_owned()],
        &BTreeMap::new(),
        None,
        "generation-busy-race",
    )
    .await
    .expect("managed session");
    let mut events = launched.protocol.subscribe();

    let error = launched
        .protocol
        .start_prompt(FAKE_SESSION_ID, "delivery-1", "retry me")
        .await
        .expect_err("provider-side busy rejection");
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(
        events.try_recv().is_err(),
        "a rejected prompt was not admitted"
    );

    let turn_id = launched
        .protocol
        .start_prompt(FAKE_SESSION_ID, "delivery-1", "retry me")
        .await
        .expect("same delivery is accepted after the provider becomes idle");
    let started = next_method(&mut events, "turn/started").await;
    assert_eq!(started.message["params"]["turn"]["id"], turn_id);
    assert_eq!(
        started.requested_delegation_id.as_deref(),
        Some("delivery-1")
    );
    assert_eq!(
        next_method(&mut events, "turn/completed").await.message["params"]["turn"]["id"],
        turn_id
    );
    stop(launched).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn voice_admission_falls_back_to_the_native_runtime_after_a_busy_race() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("cccc-home")).expect("home");
    home.initialize().expect("initialize");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    let executable = fake_grok_busy_once(temp.path());
    let launched = launch(
        &home,
        &workspace,
        &[executable.to_string_lossy().into_owned()],
        &BTreeMap::new(),
        None,
        "generation-voice-busy-race",
    )
    .await
    .expect("managed session");
    let session = Arc::new(analyst_from_grok(
        launched,
        &workspace,
        "generation-voice-busy-race",
    ));
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    let control = match &session.protocol {
        ManagedProtocol::Acp(protocol) => protocol.lifecycle_control(),
        ManagedProtocol::Codex(_) | ManagedProtocol::Claude(_) => {
            panic!("expected ACP protocol")
        }
    };

    assert_eq!(
        lifecycle
            .admit_voice("voice-busy-race", "preserve this follow-up")
            .await
            .expect("native fallback admission"),
        VoiceDelegationAdmission::NativeInput {
            delegation_id: "voice-busy-race".into(),
            text: "preserve this follow-up".into(),
        }
    );
    assert!(
        control
            .user_text(FAKE_SESSION_ID, "preserve this follow-up")
            .await
            .expect("authoritative native input echo")
    );
    assert!(matches!(
        events.recv().await.expect("Voice turn start"),
        AnalystLifecycleEvent::Started {
            ref receipt,
            origin: crate::ops::codex_voice_lifecycle::AnalystTurnOrigin::Voice,
        } if receipt.delegation_id == "voice-busy-race"
    ));

    control
        .status(FAKE_SESSION_ID, false)
        .await
        .expect("external idle");
    session
        .stop(session.generation())
        .await
        .expect("stop session");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_without_a_user_echo_replays_buffered_output_after_acceptance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = fake_acp_without_user_echo(temp.path(), false);
    let (protocol, process) =
        launch_fake_acp(temp.path(), &executable, PromptCompletion::Response).await;
    let session = Arc::new(fake_analyst_session(protocol, process, temp.path()));
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();

    let admission = lifecycle
        .admit_voice("delivery-no-echo", "owned prompt")
        .await
        .expect("prompt accepted without a user echo");
    let VoiceDelegationAdmission::Turn(turn) = admission else {
        panic!("idle Runtime must accept the controlled prompt")
    };
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let AnalystLifecycleEvent::Completed {
                turn_id,
                status,
                result,
                ..
            } = events.recv().await.expect("lifecycle event")
                && turn_id == turn.turn_id
            {
                break (status, result);
            }
        }
    })
    .await
    .expect("completed lifecycle result");
    assert_eq!(result, ("completed".into(), "buffered result".into()));

    session
        .stop(session.generation())
        .await
        .expect("stop fake Analyst");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn response_fenced_provider_cannot_silently_complete_before_its_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = fake_acp_without_user_echo(temp.path(), true);
    let (protocol, process) =
        launch_fake_acp(temp.path(), &executable, PromptCompletion::Response).await;
    let session = Arc::new(fake_analyst_session(protocol, process, temp.path()));
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();

    let admission = lifecycle
        .admit_voice("delivery-late-result", "owned prompt")
        .await
        .expect("prompt response accepted");
    let VoiceDelegationAdmission::Turn(turn) = admission else {
        panic!("idle Runtime must accept the controlled prompt")
    };
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let AnalystLifecycleEvent::Completed {
                turn_id,
                status,
                result,
                ..
            } = events.recv().await.expect("lifecycle event")
                && turn_id == turn.turn_id
            {
                break (status, result);
            }
        }
    })
    .await
    .expect("completed lifecycle result");
    assert_eq!(result, ("failed".into(), String::new()));
    assert!(
        tokio::time::timeout(Duration::from_millis(250), events.recv())
            .await
            .is_err(),
        "post-completion output must not be attached to the finished turn"
    );

    session
        .stop(session.generation())
        .await
        .expect("stop fake Analyst");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_specific_bounded_drain_can_preserve_immediately_late_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = fake_acp_without_user_echo(temp.path(), true);
    let (protocol, process) = launch_fake_acp(
        temp.path(),
        &executable,
        PromptCompletion::BoundedPostResponseDrain,
    )
    .await;
    let mut events = protocol.subscribe();

    let turn_id = protocol
        .start_prompt(FAKE_SESSION_ID, "delivery-late-output", "owned prompt")
        .await
        .expect("prompt response accepted");
    assert_eq!(
        next_method(&mut events, "turn/started").await.message["params"]["turn"]["id"],
        turn_id
    );
    assert_eq!(
        next_method(&mut events, "item/agentMessage/delta")
            .await
            .message["params"]["delta"],
        "late result"
    );
    assert_eq!(
        next_method(&mut events, "turn/completed").await.message["params"]["turn"]["id"],
        turn_id
    );

    protocol.close().await;
    process.stop().expect("stop fake ACP");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_control_updates_the_managed_acp_model() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (executable, observed_request) = fake_acp_config_option(temp.path());
    let (protocol, process) =
        launch_fake_acp(temp.path(), &executable, PromptCompletion::Response).await;

    protocol
        .lifecycle_control()
        .set_config_option(FAKE_SESSION_ID, "model", "anthropic/claude-sonnet-4/high")
        .await
        .expect("update managed ACP model");

    let request: Value = serde_json::from_str(
        &std::fs::read_to_string(observed_request).expect("recorded config request"),
    )
    .expect("config request JSON");
    assert_eq!(request["method"], "session/set_config_option");
    assert_eq!(request["params"]["sessionId"], FAKE_SESSION_ID);
    assert_eq!(request["params"]["configId"], "model");
    assert_eq!(request["params"]["value"], "anthropic/claude-sonnet-4/high");

    protocol.close().await;
    process.stop().expect("stop fake ACP");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cccc_cancel_intent_normalizes_an_end_turn_response() {
    let temp = tempfile::tempdir().expect("tempdir");
    let executable = fake_acp_cancel_as_end_turn(temp.path());
    let (protocol, process) =
        launch_fake_acp(temp.path(), &executable, PromptCompletion::Response).await;
    let mut events = protocol.subscribe();
    let lifecycle = protocol.lifecycle_control();

    let admission = async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        lifecycle
            .user_text(FAKE_SESSION_ID, "cancel me")
            .await
            .expect("authoritative backend user message")
    };
    let (turn_id, native_input) = tokio::join!(
        protocol.start_prompt(FAKE_SESSION_ID, "delivery-cancel", "cancel me"),
        admission,
    );
    assert!(!native_input);
    let turn_id = turn_id.expect("prompt admitted from backend user message");
    assert_eq!(
        next_method(&mut events, "turn/started").await.message["params"]["turn"]["id"],
        turn_id
    );
    protocol
        .cancel(FAKE_SESSION_ID)
        .await
        .expect("cancel request");
    let completed = next_method(&mut events, "turn/completed").await;
    assert_eq!(completed.message["params"]["turn"]["id"], turn_id);
    assert_eq!(completed.message["params"]["turn"]["status"], "cancelled");

    protocol.close().await;
    process.stop().expect("stop fake ACP");
}

async fn launch_fake_acp(
    root: &Path,
    executable: &Path,
    prompt_completion: PromptCompletion,
) -> (AcpClient, process::ChildOwner) {
    // These fixtures are shell source, not Runtime executables. Reading them
    // via sh avoids ETXTBSY when another concurrently spawned test process
    // briefly inherits a just-written fixture descriptor before exec.
    let command = ["/bin/sh".into(), executable.to_string_lossy().into_owned()];
    let (process, stdin, stdout) =
        process::spawn_piped(&command, root, &BTreeMap::new(), "fake-acp").expect("spawn fake ACP");
    let protocol = AcpClient::new(
        stdin,
        stdout,
        "generation-fake-acp".into(),
        "opencode",
        PermissionPolicy::Reject,
        prompt_completion,
    )
    .expect("fake ACP client");
    protocol
        .request("initialize", json!({}), Duration::from_secs(2))
        .await
        .expect("initialize fake ACP");
    protocol
        .request("session/new", json!({}), Duration::from_secs(2))
        .await
        .expect("create fake ACP session");
    (protocol, process)
}

fn fake_analyst_session(
    protocol: AcpClient,
    process: process::ChildOwner,
    root: &Path,
) -> AnalystSession {
    AnalystSession {
        binding: WorkspaceBinding {
            root: root.canonicalize().expect("fake Analyst root"),
        },
        generation: "generation-fake-acp".into(),
        runtime: ActorRuntime::Opencode,
        endpoint: String::new(),
        thread_id: FAKE_SESSION_ID.into(),
        remote_tui_prefix: Vec::new(),
        environment: BTreeMap::new(),
        protocol: ManagedProtocol::Acp(protocol),
        process: Some(Arc::new(process)),
        auxiliary_processes: Vec::new(),
        native_tui_command: None,
        cleanup_paths: Vec::new(),
        thread_resumed: false,
        delegations: tokio::sync::Mutex::new(HashMap::new()),
    }
}

fn analyst_from_grok(
    launched: grok::LaunchedGrok,
    workspace: &Path,
    generation: &str,
) -> AnalystSession {
    AnalystSession {
        binding: WorkspaceBinding {
            root: workspace.canonicalize().expect("workspace root"),
        },
        generation: generation.into(),
        runtime: ActorRuntime::Grok,
        endpoint: String::new(),
        thread_id: launched.session_id,
        remote_tui_prefix: Vec::new(),
        environment: BTreeMap::new(),
        protocol: ManagedProtocol::Acp(launched.protocol),
        process: Some(launched.process),
        auxiliary_processes: launched.auxiliary_processes,
        native_tui_command: Some(launched.tui_command),
        cleanup_paths: launched.cleanup_paths,
        thread_resumed: launched.resumed,
        delegations: tokio::sync::Mutex::new(HashMap::new()),
    }
}

async fn launch(
    home: &HomeLayout,
    workspace: &Path,
    command: &[String],
    environment: &BTreeMap<String, String>,
    resume: Option<&str>,
    generation: &str,
) -> io::Result<grok::LaunchedGrok> {
    let prepared = grok::prepare(home, command, environment, generation)?;
    grok::launch(
        prepared,
        workspace,
        environment,
        generation,
        SessionPurpose::VoiceAnalyst,
        resume,
        json!({"name":"cccc-test","command":"cccc-test","args":[]}),
    )
    .await
}

async fn next_method(
    events: &mut tokio::sync::broadcast::Receiver<super::super::AnalystEvent>,
    method: &str,
) -> super::super::AnalystEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("event timeout")
            .expect("event stream");
        if event.message.get("method").and_then(Value::as_str) == Some(method) {
            return event;
        }
    }
}

async fn stop(launched: grok::LaunchedGrok) {
    launched.protocol.close().await;
    launched.process.stop().expect("stop ACP");
    for process in launched.auxiliary_processes {
        process.stop().expect("stop leader");
    }
    for path in launched.cleanup_paths {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!("remove socket: {error}"),
        }
    }
}

fn fake_grok(root: &Path) -> PathBuf {
    let path = root.join("grok");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
case " $* " in
  *" leader "*)
    trap 'exit 0' TERM INT
    while :; do sleep 1; done
    ;;
esac
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentCapabilities":{{"loadSession":true}}}}}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"{FAKE_SESSION_ID}"}}}}'
      ;;
    *'"method":"session/load"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"{FAKE_SESSION_ID}"}}}}'
      ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{FAKE_SESSION_ID}","update":{{"sessionUpdate":"user_message_chunk","content":{{"type":"text","text":"probe"}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{FAKE_SESSION_ID}","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"fake result"}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"_x.ai/session_notification","params":{{"sessionId":"{FAKE_SESSION_ID}","update":{{"sessionUpdate":"turn_completed","stopReason":"end_turn"}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn"}}}}'
      ;;
  esac
done
"#
        ),
    )
    .expect("fake grok");
    let mut permissions = path.metadata().expect("metadata").permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("executable");
    path
}

fn fake_grok_busy_once(root: &Path) -> PathBuf {
    let directory = root.join("busy-once");
    std::fs::create_dir(&directory).expect("fake Grok directory");
    let path = directory.join("grok");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
case " $* " in
  *" leader "*)
    trap 'exit 0' TERM INT
    while :; do sleep 1; done
    ;;
esac
prompt_count=0
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentCapabilities":{{"loadSession":true}}}}}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"{FAKE_SESSION_ID}"}}}}'
      ;;
    *'"method":"session/prompt"'*)
      prompt_count=$((prompt_count + 1))
      if [ "$prompt_count" -eq 1 ]; then
        printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{FAKE_SESSION_ID}","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"foreign result"}}}}}}}}'
        printf '%s\n' '{{"jsonrpc":"2.0","id":3,"error":{{"code":-32603,"message":"session is busy"}}}}'
      else
        printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{FAKE_SESSION_ID}","update":{{"sessionUpdate":"user_message_chunk","content":{{"type":"text","text":"retry me"}}}}}}}}'
        printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"stopReason":"end_turn"}}}}'
      fi
      ;;
  esac
done
"#
        ),
    )
    .expect("fake busy Grok");
    let mut permissions = path.metadata().expect("metadata").permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("executable");
    path
}

fn fake_acp_without_user_echo(root: &Path, response_before_output: bool) -> PathBuf {
    let directory = root.join(if response_before_output {
        "response-before-output"
    } else {
        "output-before-response"
    });
    std::fs::create_dir(&directory).expect("fake ACP directory");
    let path = directory.join("fake-acp");
    let prompt = if response_before_output {
        format!(
            r#"printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn"}}}}'
sleep 0.05
printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{FAKE_SESSION_ID}","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"late result"}}}}}}}}'"#
        )
    } else {
        format!(
            r#"printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{FAKE_SESSION_ID}","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"buffered result"}}}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn"}}}}'"#
        )
    };
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1}}}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"{FAKE_SESSION_ID}"}}}}'
      ;;
    *'"method":"session/prompt"'*)
      {prompt}
      ;;
  esac
done
"#
        ),
    )
    .expect("fake ACP script");
    let mut permissions = path.metadata().expect("metadata").permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("executable");
    path
}

fn fake_acp_cancel_as_end_turn(root: &Path) -> PathBuf {
    let directory = root.join("cancel-as-end-turn");
    std::fs::create_dir(&directory).expect("fake ACP directory");
    let path = directory.join("fake-acp");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1}}}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"{FAKE_SESSION_ID}"}}}}'
      ;;
    *'"method":"session/prompt"'*)
      :
      ;;
    *'"method":"session/cancel"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn"}}}}'
      ;;
  esac
done
"#
        ),
    )
    .expect("fake cancelling ACP script");
    let mut permissions = path.metadata().expect("metadata").permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("executable");
    path
}

fn fake_acp_config_option(root: &Path) -> (PathBuf, PathBuf) {
    let directory = root.join("config-option");
    std::fs::create_dir(&directory).expect("fake ACP directory");
    let path = directory.join("fake-acp");
    let observed_request = directory.join("request.json");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1}}}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"{FAKE_SESSION_ID}"}}}}'
      ;;
    *'"method":"session/set_config_option"'*)
      printf '%s\n' "$line" > '{}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{}}}}'
      ;;
  esac
done
"#,
            observed_request.display(),
        ),
    )
    .expect("fake config ACP script");
    let mut permissions = path.metadata().expect("metadata").permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).expect("executable");
    (path, observed_request)
}
