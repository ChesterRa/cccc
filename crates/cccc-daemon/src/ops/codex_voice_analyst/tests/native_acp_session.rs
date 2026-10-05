use super::super::*;
use cccc_contracts::RuntimeMode;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

fn fixture(runtime: ActorRuntime) -> (tempfile::TempDir, HomeLayout, String) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    home.initialize().expect("initialize");
    let program = temp.path().join(match runtime {
        ActorRuntime::Cursor => "cursor-agent",
        _ => native_acp::name(runtime),
    });
    std::fs::write(&program, include_str!("native_acp_fixture.py")).expect("script");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("executable");
    (temp, home, program.to_string_lossy().into_owned())
}
fn environment(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "HOME".into(),
            root.join("provider").to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".into(),
            root.join("provider/config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            root.join("provider/data").to_string_lossy().into_owned(),
        ),
        ("ACP_FIXTURE_CLI".into(), "/bin/true".into()),
    ])
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
async fn wait_interaction(session: &AnalystSession) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Some(request) = session.permissions().first() {
                return request.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("interaction")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_model_is_applied_after_both_new_and_load_and_rejection_is_visible() {
    for (runtime, requested, selected, method, field) in [
        (
            ActorRuntime::Cursor,
            "fixture-model",
            "fixture-model[fast=true]",
            "session/set_model",
            "modelId",
        ),
        (
            ActorRuntime::Devin,
            "Fixture model",
            "fixture-model-medium",
            "session/set_config_option",
            "value",
        ),
    ] {
        let (temp, home, program) = fixture(runtime);
        let mut config = LaunchConfig::new(temp.path());
        config.runtime = runtime;
        config.runtime_mode = RuntimeMode::Acp;
        config.command = vec![program, "--model".into(), requested.into()];
        config.environment = environment(temp.path());
        let session = AnalystSession::launch(&home, config.clone())
            .await
            .expect("new");
        let id = session.thread_id().to_owned();
        cccc_core::fs::write_json(
            &home.daemon_dir().join("codex_voice_analyst.json"),
            &json!({"thread_id":id,"materialized":false}),
        )
        .expect("Voice host receipt");
        let mut events = session.subscribe();
        session
            .start_turn(session.generation(), "materialize", "fixture")
            .await
            .expect("admission");
        terminal(&mut events).await;
        session.stop(session.generation()).await.expect("stop");
        config.resume_thread_id = Some(id.clone());
        let resumed = AnalystSession::launch(&home, config.clone())
            .await
            .expect("load");
        assert_eq!(resumed.thread_id(), id);
        resumed.stop(resumed.generation()).await.expect("stop");
        let frames =
            std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("frames");
        let selections: Vec<Value> = frames
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("fixture frame"))
            .filter(|frame| frame["method"] == method)
            .collect();
        assert_eq!(selections.len(), 2);
        assert!(
            selections
                .iter()
                .all(|frame| frame["params"][field] == selected)
        );
        config.command[2] = "unavailable".into();
        assert!(AnalystSession::launch(&home, config).await.is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_acp_actors_isolate_empty_and_attempted_sessions_and_inject_mcp() {
    for runtime in [
        ActorRuntime::Copilot,
        ActorRuntime::Devin,
        ActorRuntime::Cursor,
    ] {
        let (temp, home, program) = fixture(runtime);
        let group = cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .create("fixture", "")
            .expect("group");
        let launch = |id: &str| ActorLaunchConfig {
            workdir: temp.path().into(),
            group_id: group.group_id.clone(),
            actor_id: id.into(),
            runtime,
            runtime_mode: RuntimeMode::Acp,
            command: vec![program.clone()],
            environment: environment(temp.path()),
        };
        let empty = AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .expect("empty");
        assert!(empty.structured_only());
        assert!(!empty.tui_ready());
        let empty_id = empty.thread_id().to_owned();
        empty.stop(empty.generation()).await.expect("stop empty");
        let first = AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .expect("replace never-attempted empty");
        assert_ne!(first.thread_id(), empty_id);
        let second = AnalystSession::launch_actor(&home, launch("beta"))
            .await
            .expect("second actor");
        assert_ne!(first.thread_id(), second.thread_id());
        let mut events = first.subscribe();
        first
            .start_turn(first.generation(), "delivery-1", "fixture task")
            .await
            .expect("turn");
        assert_eq!(
            terminal(&mut events).await["params"]["turn"]["status"],
            "completed"
        );
        let id = first.thread_id().to_owned();
        first.stop(first.generation()).await.expect("stop");
        let resumed = AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .expect("resume populated");
        assert_eq!(resumed.thread_id(), id);
        assert!(resumed.thread_resumed);
        resumed.stop(resumed.generation()).await.expect("stop");
        second.stop(second.generation()).await.expect("stop second");
        std::fs::write(temp.path().join("fixture_reject_load"), "").expect("force load failure");
        assert!(
            AnalystSession::launch_actor(&home, launch("alpha"))
                .await
                .is_err()
        );
        let frames =
            std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("frames");
        assert_eq!(
            frames.matches("\"method\": \"session/new\"").count(),
            3,
            "resume failure must not replace attempted work"
        );
        let requests: Vec<Value> = frames
            .lines()
            .map(|line| serde_json::from_str(line).expect("json"))
            .collect();
        if runtime != ActorRuntime::Copilot {
            for id in ["alpha", "beta"] {
                assert!(requests.iter().any(|frame| {
                    frame["params"]["mcpServers"][0]["env"]
                        .as_array()
                        .is_some_and(|env| {
                            env.iter().any(|entry| {
                                entry["name"] == "CCCC_ACTOR_ID" && entry["value"] == id
                            })
                        })
                }));
            }
        } else {
            let launches =
                std::fs::read_to_string(temp.path().join("fixture_launch.jsonl")).expect("argv");
            let arguments: Vec<Vec<String>> = launches
                .lines()
                .map(|line| serde_json::from_str(line).expect("argv json"))
                .collect();
            for id in ["alpha", "beta"] {
                assert!(arguments.iter().any(|argv| {
                    argv
                        .iter()
                        .position(|arg| arg == "--additional-mcp-config")
                        .is_some_and(|at| serde_json::from_str::<Value>(&argv[at + 1])
                            .expect("mcp json")["mcpServers"]["cccc"]["env"]["CCCC_ACTOR_ID"]
                            == id)
                }));
            }
            assert!(
                !temp
                    .path()
                    .join("provider/.copilot/mcp-config.json")
                    .exists()
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cursor_yolo_still_waits_for_explicit_answers_and_plan_decisions() {
    let (temp, home, program) = fixture(ActorRuntime::Cursor);
    let mut config = LaunchConfig::new(temp.path());
    config.runtime = ActorRuntime::Cursor;
    config.runtime_mode = RuntimeMode::Acp;
    config.command = vec![program, "--yolo".into(), "--model=fixture-model".into()];
    config.environment = environment(temp.path());
    let session = AnalystSession::launch(&home, config).await.expect("launch");
    // The Web host persists a non-materialized binding before accepting input.
    cccc_core::fs::write_json(
        &home.daemon_dir().join("codex_voice_analyst.json"),
        &json!({"thread_id":session.thread_id(),"materialized":false}),
    )
    .expect("host receipt");
    assert!(!session.resumable());
    let mut events = session.subscribe();
    session
        .start_turn(session.generation(), "question", "QUESTION")
        .await
        .expect("admission");
    assert!(session.resumable());
    let question = wait_interaction(&session).await;
    assert_eq!(question["kind"], "question");
    let request = question["request_id"].as_str().expect("request");
    assert!(
        session
            .respond_permission(session.generation(), request, true)
            .await
            .is_err(),
        "YOLO cannot answer a question"
    );
    assert!(
        session
            .respond_interaction("stale", request, json!({"outcome":{"outcome":"skipped"}}))
            .await
            .is_err()
    );
    assert!(
        session
            .respond_interaction(
                session.generation(),
                request,
                json!({"outcome":{"outcome":"answered","answers":[]}})
            )
            .await
            .is_err()
    );
    session.respond_interaction(session.generation(),request,json!({"outcome":{"outcome":"answered","answers":[{"questionId":"q1","selectedOptionIds":["b"]},{"questionId":"q2","selectedOptionIds":["c","d"]}]}})).await.expect("human answer");
    terminal(&mut events).await;
    assert!(
        session
            .respond_interaction(
                session.generation(),
                request,
                json!({"outcome":{"outcome":"skipped"}})
            )
            .await
            .is_err(),
        "expired request"
    );
    session
        .start_turn(session.generation(), "plan", "PLAN")
        .await
        .expect("plan admission");
    let plan = wait_interaction(&session).await;
    assert_eq!(plan["kind"], "plan");
    session
        .respond_interaction(
            session.generation(),
            plan["request_id"].as_str().expect("plan id"),
            json!({"outcome":{"outcome":"rejected"}}),
        )
        .await
        .expect("reject plan");
    terminal(&mut events).await;
    session
        .start_turn(session.generation(), "cancel-question", "QUESTION")
        .await
        .expect("question again");
    let pending = wait_interaction(&session).await;
    session
        .interrupt(session.generation(), "ignored-by-acp")
        .await
        .expect("cancel");
    terminal(&mut events).await;
    assert!(
        session
            .respond_interaction(
                session.generation(),
                pending["request_id"].as_str().expect("id"),
                json!({"outcome":{"outcome":"skipped"}})
            )
            .await
            .is_err()
    );
    session.stop(session.generation()).await.expect("stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reused_provider_ids_never_accept_retired_interaction_replies() {
    for (runtime, text) in [
        (ActorRuntime::Cursor, "QUESTION"),
        (ActorRuntime::Cursor, "PLAN"),
        (ActorRuntime::Cursor, "PERMISSION"),
        (ActorRuntime::Copilot, "PERMISSION"),
        (ActorRuntime::Devin, "PERMISSION"),
    ] {
        let (temp, home, program) = fixture(runtime);
        let mut config = LaunchConfig::new(temp.path());
        config.runtime = runtime;
        config.runtime_mode = RuntimeMode::Acp;
        config.command = vec![program];
        config.environment = environment(temp.path());
        let session = AnalystSession::launch(&home, config).await.expect("launch");
        cccc_core::fs::write_json(
            &home.daemon_dir().join("codex_voice_analyst.json"),
            &json!({"thread_id":session.thread_id(),"materialized":false}),
        )
        .expect("isolated host receipt");
        let mut events = session.subscribe();
        session
            .start_turn(session.generation(), "old-request", text)
            .await
            .expect("old admission");
        let old = wait_interaction(&session).await;
        session
            .interrupt(session.generation(), "ignored-by-acp")
            .await
            .expect("cancel old request");
        terminal(&mut events).await;
        session
            .start_turn(session.generation(), "replacement-request", text)
            .await
            .expect("replacement admission");
        let replacement = wait_interaction(&session).await;
        let reply = match text {
            "QUESTION" => json!({"outcome":{"outcome":"skipped"}}),
            "PLAN" => json!({"outcome":{"outcome":"accepted"}}),
            _ => json!({"allow":true}),
        };
        let stale_reply = session
            .respond_interaction(
                session.generation(),
                old["request_id"].as_str().expect("old token"),
                reply.clone(),
            )
            .await;
        // The fixture deliberately reuses the wire RPC id in the next turn.
        assert!(
            stale_reply.is_err(),
            "stale {runtime:?}/{text} reply accepted"
        );
        assert_ne!(old["request_id"], replacement["request_id"]);
        assert_eq!(session.permissions(), vec![replacement.clone()]);
        session
            .respond_interaction(
                session.generation(),
                replacement["request_id"]
                    .as_str()
                    .expect("replacement token"),
                reply,
            )
            .await
            .expect("reply to current request");
        assert_eq!(
            terminal(&mut events).await["params"]["turn"]["status"],
            "completed"
        );
        session.stop(session.generation()).await.expect("stop");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnect_before_admission_keeps_attempted_receipt() {
    let (temp, home, program) = fixture(ActorRuntime::Devin);
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("fixture", "")
        .expect("group");
    let launch = || ActorLaunchConfig {
        workdir: temp.path().into(),
        group_id: group.group_id.clone(),
        actor_id: "worker".into(),
        runtime: ActorRuntime::Devin,
        runtime_mode: RuntimeMode::Acp,
        command: vec![program.clone()],
        environment: environment(temp.path()),
    };
    let session = AnalystSession::launch_actor(&home, launch())
        .await
        .expect("session");
    assert!(
        session
            .start_turn(
                session.generation(),
                "uncertain",
                "DISCONNECT_BEFORE_RECEIPT"
            )
            .await
            .is_err()
    );
    let receipt = crate::ops::runtime_session::native_acp::receipt_path(
        &home,
        &group.group_id,
        "worker",
        ActorRuntime::Devin,
    )
    .expect("path");
    assert_eq!(
        cccc_core::fs::read_json::<Value>(&receipt).expect("receipt")["attempted"],
        true
    );
    session.stop(session.generation()).await.expect("stop");
    std::fs::write(temp.path().join("fixture_reject_load"), "").expect("flag");
    assert!(AnalystSession::launch_actor(&home, launch()).await.is_err());
    let frames =
        std::fs::read_to_string(temp.path().join("fixture_requests.jsonl")).expect("frames");
    assert_eq!(frames.matches("\"method\": \"session/new\"").count(), 1);
    assert_eq!(frames.matches("\"method\": \"session/prompt\"").count(), 1);
}
