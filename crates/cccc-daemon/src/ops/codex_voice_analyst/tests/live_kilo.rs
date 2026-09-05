use super::super::*;
use super::live_support::{wait_for_turn_status, wait_for_turn_text};
use futures_util::{FutureExt, StreamExt};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Native Kilo + real ACP/HTTP, with a local model and isolated provider state.
/// No user credentials or paid inference. Actor and Analyst use the same probe.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_kilo_shared_actor_and_analyst_when_enabled() {
    if std::env::var("CCCC_KILO_MANAGED_LIVE").as_deref() != Ok("1") {
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local model");
    let endpoint = format!(
        "http://{}/v1",
        listener.local_addr().expect("model address")
    );
    let model_server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route("/v1/chat/completions", axum::routing::post(local_model)),
        )
        .await
        .expect("serve local model");
    });
    let observed = std::panic::AssertUnwindSafe(async {
        for purpose in [SessionPurpose::Actor, SessionPurpose::VoiceAnalyst] {
            let temp = tempfile::tempdir().expect("isolated provider home");
            let home = HomeLayout::from_path(temp.path().join("cccc")).expect("CCCC home");
            home.initialize().expect("initialize CCCC home");
            let group = cccc_core::GroupStore::new(home.clone())
                .expect("group store")
                .create("Kilo probe", "")
                .expect("create group");
            let root = temp.path().join("project");
            std::fs::create_dir_all(&root).expect("project directory");
            let mut environment = BTreeMap::from([
                ("HOME".into(), temp.path().to_string_lossy().into_owned()),
                (
                    "KILO_DB".into(),
                    temp.path().join("kilo.db").to_string_lossy().into_owned(),
                ),
                ("TERM".into(), "xterm-256color".into()),
                (
                    "KILO_CONFIG_CONTENT".into(),
                    json!({
                        "model":"cccc-test/first", "small_model":"cccc-test/first",
                        "provider":{"cccc-test":{
                            "npm":"@ai-sdk/openai-compatible",
                            "options":{"baseURL":endpoint,"apiKey":"local-test"},
                            "models":{"first":{"name":"First"}}
                        }}
                    })
                    .to_string(),
                ),
            ]);
            for part in ["CONFIG", "DATA", "STATE", "CACHE"] {
                environment.insert(
                    format!("XDG_{part}_HOME"),
                    temp.path()
                        .join(part.to_lowercase())
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            let command = vec![
                std::env::var("CCCC_KILO_EXECUTABLE").unwrap_or_else(|_| "kilo".into()),
                "--pure".into(),
            ];
            let mut previous = None;
            for cycle in 0..3 {
                eprintln!("Kilo {purpose:?} cycle {cycle}");
                let session = match purpose {
                    SessionPurpose::Actor => {
                        AnalystSession::launch_actor(
                            &home,
                            ActorLaunchConfig {
                                workdir: root.clone(),
                                group_id: group.group_id.clone(),
                                actor_id: "kilo-test".into(),
                                runtime: ActorRuntime::Kilo,
                                command: command.clone(),
                                environment: environment.clone(),
                            },
                        )
                        .await
                    }
                    SessionPurpose::VoiceAnalyst => {
                        AnalystSession::launch(
                            &home,
                            LaunchConfig {
                                workdir: root.clone(),
                                runtime: ActorRuntime::Kilo,
                                command: command.clone(),
                                environment: environment.clone(),
                                resume_thread_id: previous.clone(),
                            },
                        )
                        .await
                    }
                }
                .expect("launch managed Kilo");
                let result = std::panic::AssertUnwindSafe(async {
                    if let Some(id) = &previous {
                        assert_eq!(session.thread_id(), id);
                        assert!(session.thread_resumed);
                    }
                    let mut events = session.subscribe();
                    cccc_runtime::start(cccc_runtime::LaunchSpec {
                        group_id: group.group_id.clone(),
                        actor_id: "kilo-test".into(),
                        runner: cccc_contracts::RunnerKind::Pty,
                        command: session.actor_tui_command(),
                        cwd: root.clone(),
                        env: session.tui_environment(),
                        cols: 120,
                        rows: 40,
                    })
                    .expect("native TUI");
                    assert!(
                        tokio::task::block_in_place(|| cccc_runtime::wait_for_input_ready(
                            &group.group_id,
                            "kilo-test",
                            Duration::from_secs(15),
                            &AtomicBool::new(false)
                        ))
                        .expect("native input readiness")
                    );
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    while let Ok(event) = events.try_recv() {
                        assert_ne!(
                            event.message["method"], "turn/started",
                            "startup must remain idle"
                        );
                        assert_ne!(event.message["method"], MANAGED_AGENT_DISCONNECTED_METHOD);
                    }
                    if cycle == 1 {
                        for index in 0..3 {
                            let turn = session
                                .start_turn(
                                    session.generation(),
                                    &format!("probe-{index}"),
                                    "Say hello.",
                                )
                                .await
                                .expect("prompt result");
                            assert_eq!(
                                wait_for_turn_text(&mut events, &turn.turn_id).await,
                                "KILO_FINAL"
                            );
                        }
                        let turn = session
                            .start_turn(session.generation(), "cancel-probe", "SLOW")
                            .await
                            .expect("cancellable prompt");
                        // The native TUI must cancel the same ACP-originated work.
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        cccc_runtime::write(&group.group_id, "kilo-test", b"\x1b")
                            .expect("first Esc");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        cccc_runtime::write(&group.group_id, "kilo-test", b"\x1b")
                            .expect("second Esc");
                        assert_eq!(
                            wait_for_turn_status(&mut events, &turn.turn_id).await,
                            "cancelled"
                        );

                        // Busy Voice follow-ups use the same native input path as Actors.
                        // A long paste also verifies that readiness precedes payload delivery.
                        session
                            .start_turn(session.generation(), "busy-probe", "QUEUE_SLOW")
                            .await
                            .expect("busy prompt");
                        let followup = format!("{} NATIVE_FOLLOWUP", "detail ".repeat(2400));
                        session
                            .register_native_input(
                                session.generation(),
                                "native-followup",
                                &followup,
                            )
                            .await
                            .expect("register follow-up");
                        cccc_runtime::write(
                            &group.group_id,
                            "kilo-test",
                            format!("\x1b[200~{followup}\x1b[201~").as_bytes(),
                        )
                        .expect("paste follow-up");
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        cccc_runtime::write(&group.group_id, "kilo-test", b"\r")
                            .expect("submit follow-up");
                        let native_turn = tokio::time::timeout(Duration::from_secs(30), async {
                            loop {
                                let event = events.recv().await.expect("native event");

                                if event.requested_delegation_id.as_deref()
                                    == Some("native-followup")
                                {
                                    let id = if event.message["method"] == "turn/started" {
                                        &event.message["params"]["turn"]["id"]
                                    } else {
                                        &event.message["params"]["turnId"]
                                    };
                                    break id.as_str().expect("correlated turn id").to_owned();
                                }
                            }
                        })
                        .await
                        .expect("native follow-up must be correlated");
                        assert!(
                            wait_for_turn_text(&mut events, &native_turn)
                                .await
                                .contains("KILO_NATIVE_FINAL"),
                            "the correlated result must include the follow-up answer"
                        );
                    }
                })
                .catch_unwind()
                .await;

                previous = Some(session.thread_id().to_owned());
                let _ = cccc_runtime::stop(&group.group_id, "kilo-test");
                session.stop(session.generation()).await.expect("stop Kilo");
                result.expect("Kilo shared-session assertions");
            }
        }
    })
    .catch_unwind()
    .await;
    model_server.abort();
    observed.expect("Kilo admission probe");
}

async fn local_model(axum::Json(body): axum::Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let input = body["messages"]
        .as_array()
        .and_then(|items| items.last())
        .map(ToString::to_string)
        .unwrap_or_default();
    let delay = if input.contains("QUEUE_SLOW") {
        6
    } else if input.contains("SLOW") {
        60
    } else {
        0
    };

    let content = if input.contains("NATIVE_FOLLOWUP") {
        "KILO_NATIVE_FINAL"
    } else {
        "KILO_FINAL"
    };
    let delta = json!({"id":"probe","object":"chat.completion.chunk","created":1,"model":"first","choices":[{"index":0,"delta":{"role":"assistant","content":content},"finish_reason":null}]});
    let end = json!({"id":"probe","object":"chat.completion.chunk","created":1,"model":"first","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
    let first = futures_util::stream::once(async move {
        Ok::<_, std::convert::Infallible>(format!("data: {delta}\n\n"))
    });
    let last = futures_util::stream::once(async move {
        if delay > 0 {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        Ok::<_, std::convert::Infallible>(format!("data: {end}\n\ndata: [DONE]\n\n"))
    });
    (
        [("content-type", "text/event-stream")],
        axum::body::Body::from_stream(first.chain(last)),
    )
        .into_response()
}
