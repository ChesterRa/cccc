use super::super::*;
use super::support::{fake_app_server, fake_disconnecting_app_server};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[tokio::test]
async fn new_codex_sessions_fail_before_attach_if_durable_history_is_unavailable() {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio_tungstenite::tungstenite::Message;

    for purpose in [SessionPurpose::Actor, SessionPurpose::VoiceAnalyst] {
        for failure in [
            "thread/name/set",
            "thread/read",
            "mismatched history",
            "missing history",
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("listener");
            let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut socket = tokio_tungstenite::accept_async(stream)
                    .await
                    .expect("websocket");
                for method in [
                    "initialize",
                    "thread/start",
                    "thread/name/set",
                    "thread/read",
                ] {
                    let Message::Text(text) = socket.next().await.expect("frame").expect("text")
                    else {
                        panic!("expected request");
                    };
                    let request: Value = serde_json::from_str(&text).expect("request");
                    assert_eq!(request["method"], method);
                    let response = if method == failure {
                        json!({"id":request["id"],"error":{"code":-32600,"message":"rollout unavailable"}})
                    } else {
                        let result = match method {
                            "thread/start" => {
                                json!({"thread":{"id":"empty-thread","path":"planned-only.jsonl"}})
                            }
                            "thread/read" if failure == "mismatched history" => {
                                json!({"thread":{"id":"other-thread","turns":[]}})
                            }
                            "thread/read" => json!({"thread":{"id":"empty-thread"}}),
                            _ => json!({}),
                        };
                        json!({"id":request["id"],"result":result})
                    };
                    socket
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .expect("response");
                    if method == failure {
                        break;
                    }
                }
            });
            let result = AnalystSession::connect(ConnectConfig {
                binding: WorkspaceBinding {
                    root: std::env::current_dir().expect("cwd"),
                },
                generation: "failed-persistence".into(),
                endpoint,
                remote_tui_prefix: vec!["codex".into()],
                environment: Default::default(),
                resume_thread_id: None,
                process: None,
                delegations: HashMap::new(),
                purpose,
            })
            .await;
            assert!(
                result.is_err(),
                "{purpose:?}: {failure} must not expose an attachable session"
            );
            server.await.expect("server");
        }
    }
}

#[tokio::test]
async fn one_delegation_maps_to_one_turn_and_supports_steer_interrupt_and_tui() {
    let (endpoint, server, turn_starts, elicitation_response) = fake_app_server().await;
    let binding = WorkspaceBinding {
        root: std::env::current_dir().expect("cwd"),
    };
    let session = AnalystSession::connect(ConnectConfig {
        binding,
        generation: "generation-a".into(),
        endpoint: endpoint.clone(),
        remote_tui_prefix: vec![
            "codex".into(),
            "-c".into(),
            "model_provider=\"ZAI\"".into(),
            "--model".into(),
            "glm-5.3".into(),
        ],
        environment: Default::default(),
        resume_thread_id: None,
        process: None,
        delegations: HashMap::new(),
        purpose: SessionPurpose::VoiceAnalyst,
    })
    .await
    .expect("connect");
    assert!(
        session.tui_ready(),
        "thread creation must make the native TUI attachable without a model turn"
    );
    assert_eq!(
        turn_starts.load(Ordering::SeqCst),
        0,
        "launching a managed session must not manufacture model work"
    );
    assert_eq!(
        session.actor_tui_command(),
        session.tui_command(),
        "a fresh Actor terminal must attach to the controller-created thread",
    );
    let mut events = session.subscribe();

    let first = session
        .start_turn("generation-a", "delegation-1", "inspect the repository")
        .await
        .expect("first turn");
    let replay = session
        .start_turn("generation-a", "delegation-1", "duplicate text is ignored")
        .await
        .expect("deduped turn");
    assert_eq!(first, replay);
    assert_eq!(turn_starts.load(Ordering::SeqCst), 1);
    assert_eq!(first.turn_id, "turn-1");

    let completion = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.message["method"] == "turn/completed" {
                break event;
            }
        }
    })
    .await
    .expect("completion event");
    assert_eq!(completion.generation, "generation-a");
    assert_eq!(completion.message["params"]["turn"]["id"], "turn-1");

    session
        .steer("generation-a", &first.turn_id, "also inspect the tests")
        .await
        .expect("steer");
    session
        .interrupt("generation-a", &first.turn_id)
        .await
        .expect("interrupt");
    let elicitation = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.message["method"] == "mcpServer/elicitation/request" {
                break event;
            }
        }
    })
    .await
    .expect("elicitation event");
    session
        .respond_mcp_elicitation("generation-a", &elicitation, ElicitationAction::Accept)
        .await
        .expect("elicitation response");
    assert_eq!(
        session
            .start_turn("stale-generation", "delegation-2", "must fail")
            .await
            .expect_err("stale generation")
            .kind(),
        io::ErrorKind::InvalidInput
    );

    let response_deadline = Instant::now() + Duration::from_secs(2);
    while elicitation_response
        .lock()
        .expect("elicitation response lock")
        .is_none()
        && Instant::now() < response_deadline
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        elicitation_response
            .lock()
            .expect("elicitation response lock")
            .as_ref()
            .expect("elicitation response")["result"]["action"],
        "accept"
    );
    assert_eq!(
        session.tui_command(),
        vec![
            "codex",
            "-c",
            "model_provider=\"ZAI\"",
            "--model",
            "glm-5.3",
            "--remote",
            endpoint.as_str(),
            "resume",
            "thread-1",
            "--no-alt-screen"
        ]
    );
    let session = session
        .reconnect("generation-a")
        .await
        .expect("reconnect session");
    assert_ne!(session.generation(), "generation-a");
    assert_eq!(session.thread_id(), "thread-1");
    assert!(
        session
            .tui_command()
            .windows(2)
            .any(|pair| pair == ["-c", "model_provider=\"ZAI\""])
    );
    assert_eq!(
        session
            .start_turn(
                session.generation(),
                "delegation-1",
                "must remain deduplicated"
            )
            .await
            .expect("replayed receipt"),
        first
    );
    let second = session
        .start_turn(session.generation(), "delegation-2", "new work")
        .await
        .expect("new turn after reconnect");
    assert_eq!(second.turn_id, "turn-2");
    assert_eq!(turn_starts.load(Ordering::SeqCst), 2);
    session
        .stop(session.generation())
        .await
        .expect("stop session");
    server.await.expect("fake server");
}

#[tokio::test]
async fn disconnected_start_is_reported_and_the_ambiguous_delegation_is_not_replayed() {
    let (endpoint, server, turn_starts) = fake_disconnecting_app_server().await;
    let session = AnalystSession::connect(ConnectConfig {
        binding: WorkspaceBinding {
            root: std::env::current_dir().expect("cwd"),
        },
        generation: "generation-disconnect".into(),
        endpoint,
        remote_tui_prefix: vec![PathBuf::from("codex").to_string_lossy().into_owned()],
        environment: Default::default(),
        resume_thread_id: None,
        process: None,
        delegations: HashMap::new(),
        purpose: SessionPurpose::VoiceAnalyst,
    })
    .await
    .expect("connect");
    let mut events = session.subscribe();

    assert_eq!(
        session
            .start_turn(
                "generation-disconnect",
                "delegation-ambiguous",
                "the server will disconnect",
            )
            .await
            .expect_err("disconnected start")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    let disconnected = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("disconnect event");
            if event.message["method"] == super::super::MANAGED_AGENT_DISCONNECTED_METHOD {
                return event;
            }
        }
    })
    .await
    .expect("disconnect timeout");
    assert_eq!(disconnected.generation, "generation-disconnect");
    assert_eq!(
        session
            .start_turn(
                "generation-disconnect",
                "delegation-ambiguous",
                "must not be sent twice",
            )
            .await
            .expect_err("ambiguous replay must be fenced")
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(turn_starts.load(Ordering::SeqCst), 1);
    session
        .stop("generation-disconnect")
        .await
        .expect("stop disconnected session");
    server.await.expect("disconnecting fake server");
}
