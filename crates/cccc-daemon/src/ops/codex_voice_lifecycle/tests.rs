use super::events::{normalized_completion_status, tracked_work, truncate_utf8};
use super::*;
use crate::ops::codex_voice_analyst::WorkspaceBinding;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn terminal_turn_is_authoritative_and_blocks_parallel_voice_work() {
    let (session, server) = test_session().await;
    let session = Arc::new(session);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();

    session.publish_event_for_test(json!({
        "method":"turn/started",
        "params":{"threadId":"thread-lifecycle","turn":{"id":"turn-tui"}}
    }));
    let started = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("terminal start timeout")
        .expect("terminal start event");
    assert!(matches!(
        started,
        AnalystLifecycleEvent::Started {
            origin: AnalystTurnOrigin::Terminal,
            ..
        }
    ));
    assert!(
        session.tui_ready(),
        "turn start materializes the TUI thread"
    );
    assert!(
        lifecycle
            .begin_voice("voice-parallel", "must not overlap")
            .await
            .is_err()
    );

    session.publish_event_for_test(json!({
        "method":"turn/completed",
        "params":{
            "threadId":"thread-lifecycle",
            "turn":{"id":"turn-tui","status":"completed"}
        }
    }));
    let completed = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("terminal completion timeout")
        .expect("terminal completion event");
    assert!(matches!(
        completed,
        AnalystLifecycleEvent::Completed {
            speakable: false,
            ..
        }
    ));
    assert!(!lifecycle.is_busy().await);

    session
        .stop(session.generation())
        .await
        .expect("stop test session");
    server.await.expect("test app-server");
}

#[tokio::test]
async fn a_competing_tui_start_cannot_claim_a_pending_voice_delegation() {
    let (session, server) = test_session_with_competing_tui_start().await;
    let session = Arc::new(session);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();

    let starting = {
        let lifecycle = Arc::clone(&lifecycle);
        tokio::spawn(async move {
            lifecycle
                .begin_voice("voice-pending", "must remain Voice-owned")
                .await
        })
    };
    let disconnected = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("competing start timeout")
        .expect("competing start event");
    assert!(matches!(disconnected, AnalystLifecycleEvent::Disconnected));
    assert!(starting.await.expect("start task").is_err());
    assert!(!lifecycle.is_busy().await);
    server.await.expect("test app-server");
}

#[tokio::test]
async fn delayed_interrupt_does_not_block_authoritative_turn_completion() {
    let (session, server, interrupt_seen, release_interrupt) =
        test_session_with_delayed_interrupt().await;
    let session = Arc::new(session);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();

    session.publish_event_for_test(json!({
        "method":"turn/started",
        "params":{"threadId":"thread-lifecycle","turn":{"id":"turn-cancel"}}
    }));
    tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("terminal start timeout")
        .expect("terminal start event");

    let cancelling = {
        let lifecycle = Arc::clone(&lifecycle);
        tokio::spawn(async move { lifecycle.cancel_current().await })
    };
    interrupt_seen.await.expect("interrupt request");

    session.publish_event_for_test(json!({
        "method":"turn/completed",
        "params":{
            "threadId":"thread-lifecycle",
            "turn":{"id":"turn-cancel","status":"interrupted"}
        }
    }));
    let completed = tokio::time::timeout(Duration::from_millis(250), events.recv())
        .await
        .expect("completion must not wait for interrupt RPC")
        .expect("terminal completion event");
    assert!(matches!(completed, AnalystLifecycleEvent::Completed { .. }));
    assert!(!lifecycle.is_busy().await);

    release_interrupt.send(()).expect("release interrupt");
    assert!(
        cancelling
            .await
            .expect("cancellation task")
            .expect("interrupt response")
    );
    session
        .stop(session.generation())
        .await
        .expect("stop test session");
    server.await.expect("test app-server");
}

#[tokio::test]
async fn completion_before_start_response_does_not_reactivate_the_turn() {
    let (session, server, release_response) = test_session_with_fast_completion().await;
    let session = Arc::new(session);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    let starting = {
        let lifecycle = Arc::clone(&lifecycle);
        tokio::spawn(async move {
            lifecycle
                .begin_voice("voice-fast", "finish before the RPC response")
                .await
        })
    };

    tokio::time::timeout(Duration::from_secs(2), async {
        while lifecycle.terminal_input_allowed().await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending start must gate terminal input");

    release_response.send(()).expect("release start response");

    let started = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("fast start timeout")
        .expect("fast start event");
    assert!(matches!(started, AnalystLifecycleEvent::Started { .. }));
    let completed = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("fast completion timeout")
        .expect("fast completion event");
    assert!(matches!(completed, AnalystLifecycleEvent::Completed { .. }));

    let receipt = starting
        .await
        .expect("start task")
        .expect("completed start receipt");
    assert_eq!(receipt.turn_id, "turn-fast");
    assert!(!lifecycle.is_busy().await);

    session
        .stop(session.generation())
        .await
        .expect("stop test session");
    server.await.expect("test app-server");
}

#[tokio::test]
async fn an_unreplayable_event_gap_invalidates_the_lifecycle() {
    let (session, server) = test_session().await;
    let session = Arc::new(session);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();
    let state_guard = lifecycle.state.lock().await;
    for index in 0..3_000 {
        session.publish_event_for_test(json!({
            "method":"item/agentMessage/delta",
            "params":{"threadId":"thread-lifecycle","turnId":"turn-gap","delta":index.to_string()}
        }));
    }
    drop(state_guard);

    let disconnected = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("event gap timeout")
        .expect("event gap signal");
    assert!(matches!(disconnected, AnalystLifecycleEvent::Disconnected));
    assert!(!lifecycle.terminal_input_allowed().await);
    assert!(
        lifecycle
            .begin_voice("after-gap", "must fail closed")
            .await
            .is_err()
    );
    server.await.expect("test app-server");
}

#[tokio::test]
async fn failed_interrupt_allows_a_real_retry() {
    let (session, server, second_interrupt_seen) = test_session_with_retryable_interrupt().await;
    let session = Arc::new(session);
    let lifecycle = AnalystLifecycle::start(Arc::clone(&session));
    let mut events = lifecycle.subscribe();

    session.publish_event_for_test(json!({
        "method":"turn/started",
        "params":{"threadId":"thread-lifecycle","turn":{"id":"turn-retry-cancel"}}
    }));
    tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("retry start timeout")
        .expect("retry start event");

    assert!(lifecycle.cancel_current().await.is_err());
    assert!(lifecycle.cancel_current().await.expect("retry interrupt"));
    tokio::time::timeout(Duration::from_secs(2), second_interrupt_seen)
        .await
        .expect("second interrupt timeout")
        .expect("second interrupt signal");

    session.publish_event_for_test(json!({
        "method":"turn/completed",
        "params":{
            "threadId":"thread-lifecycle",
            "turn":{"id":"turn-retry-cancel","status":"interrupted"}
        }
    }));
    tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("retry completion timeout")
        .expect("retry completion event");
    session
        .stop(session.generation())
        .await
        .expect("stop test session");
    server.await.expect("test app-server");
}

#[test]
fn completed_text_is_utf8_bounded_and_tracked_work_is_strict() {
    let text = "界".repeat(32 * 1024);
    let bounded = truncate_utf8(&text, 32 * 1024);
    assert!(bounded.len() <= 32 * 1024);
    assert!(bounded.is_char_boundary(bounded.len()));

    let work = tracked_work(&json!({
        "method":"item/completed",
        "params":{"item":{
            "type":"mcpToolCall",
            "status":"completed",
            "server":"cccc",
            "result":{"structuredContent":{"tool_result":{
                "task_id":"T007",
                "event_id":"event-7",
                "event":{"group_id":"g_target","data":{"to":["worker"]}}
            }}}
        }}
    }))
    .expect("tracked work");
    assert_eq!(work.task_id, "T007");
    assert_eq!(work.actor_id, "worker");
    assert!(
        tracked_work(&json!({
            "method":"item/completed",
            "params":{"item":{
                "type":"mcpToolCall",
                "status":"completed",
                "server":"cccc",
                "result":{"structuredContent":{"tool_result":{
                    "task_id":"T008",
                    "event_id":"event-8",
                    "event":{"group_id":"g_target","data":{"to":["@all"]}}
                }}}
            }}
        }))
        .is_none()
    );
    assert!(
        tracked_work(&json!({
            "method":"item/completed",
            "params":{"item":{
                "type":"mcpToolCall",
                "status":"failed",
                "server":"cccc",
                "result":{"structuredContent":{
                    "task_id":"T007","event_id":"event-7","group_id":"g_target"
                }}
            }}
        }))
        .is_none()
    );
    assert!(!AnalystTurnOrigin::ActorResult { speakable: false }.speakable());
    assert!(AnalystTurnOrigin::ActorResult { speakable: true }.speakable());
    assert_eq!(
        normalized_completion_status("completed", "", AnalystTurnOrigin::Voice),
        "failed"
    );
    assert_eq!(
        normalized_completion_status("completed", "", AnalystTurnOrigin::Terminal),
        "completed"
    );
    assert_eq!(
        normalized_completion_status("completed", "answer", AnalystTurnOrigin::Voice),
        "completed"
    );
}

async fn test_session() -> (AnalystSession, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.expect("frame") else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).expect("request");
            let Some(id) = request["id"].as_u64() else {
                continue;
            };
            let result = match request["method"].as_str().expect("method") {
                "initialize" => json!({}),
                "thread/start" => json!({"thread":{"id":"thread-lifecycle"}}),
                method => panic!("unexpected lifecycle test method: {method}"),
            };
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("response");
        }
    });
    let session = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
    )
    .await
    .expect("connect test Analyst");
    (session, server)
}

async fn test_session_with_delayed_interrupt() -> (
    AnalystSession,
    JoinHandle<()>,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let (interrupt_seen_tx, interrupt_seen_rx) = oneshot::channel();
    let (release_interrupt_tx, release_interrupt_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        let mut interrupt_seen_tx = Some(interrupt_seen_tx);
        let mut release_interrupt_rx = Some(release_interrupt_rx);
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.expect("frame") else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).expect("request");
            let Some(id) = request["id"].as_u64() else {
                continue;
            };
            let result = match request["method"].as_str().expect("method") {
                "initialize" => json!({}),
                "thread/start" => json!({"thread":{"id":"thread-lifecycle"}}),
                "turn/interrupt" => {
                    interrupt_seen_tx
                        .take()
                        .expect("single interrupt")
                        .send(())
                        .expect("observe interrupt");
                    release_interrupt_rx
                        .take()
                        .expect("single interrupt gate")
                        .await
                        .expect("release interrupt response");
                    json!({})
                }
                method => panic!("unexpected lifecycle test method: {method}"),
            };
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("response");
        }
    });
    let session = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
    )
    .await
    .expect("connect test Analyst");
    (session, server, interrupt_seen_rx, release_interrupt_tx)
}

async fn test_session_with_fast_completion() -> (AnalystSession, JoinHandle<()>, oneshot::Sender<()>)
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let (release_response_tx, release_response_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        let mut release_response_rx = Some(release_response_rx);
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.expect("frame") else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).expect("request");
            let Some(id) = request["id"].as_u64() else {
                continue;
            };
            let result = match request["method"].as_str().expect("method") {
                "initialize" => json!({}),
                "thread/start" => json!({"thread":{"id":"thread-lifecycle"}}),
                "turn/start" => {
                    for event in [
                        json!({
                            "jsonrpc":"2.0",
                            "method":"turn/started",
                            "params":{"threadId":"thread-lifecycle","turn":{"id":"turn-fast"}}
                        }),
                        json!({
                            "jsonrpc":"2.0",
                            "method":"turn/completed",
                            "params":{
                                "threadId":"thread-lifecycle",
                                "turn":{"id":"turn-fast","status":"completed"}
                            }
                        }),
                    ] {
                        socket
                            .send(Message::Text(event.to_string().into()))
                            .await
                            .expect("lifecycle notification");
                    }
                    release_response_rx
                        .take()
                        .expect("single start response gate")
                        .await
                        .expect("release start response");
                    json!({"turn":{"id":"turn-fast"}})
                }
                method => panic!("unexpected lifecycle test method: {method}"),
            };
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("response");
        }
    });
    let session = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
    )
    .await
    .expect("connect test Analyst");
    (session, server, release_response_tx)
}

async fn test_session_with_competing_tui_start() -> (AnalystSession, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        while let Some(frame) = socket.next().await {
            let Ok(Message::Text(text)) = frame else {
                // The production client deliberately invalidates this conflicted session, so an
                // ungraceful peer close is the expected terminal condition for this fake server.
                break;
            };
            let request: Value = serde_json::from_str(&text).expect("request");
            let Some(id) = request["id"].as_u64() else {
                continue;
            };
            let result = match request["method"].as_str().expect("method") {
                "initialize" => json!({}),
                "thread/start" => json!({"thread":{"id":"thread-lifecycle"}}),
                "turn/start" => {
                    socket
                        .send(Message::Text(
                            json!({
                                "jsonrpc":"2.0",
                                "method":"turn/started",
                                "params":{"threadId":"thread-lifecycle","turn":{"id":"turn-tui"}}
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .expect("competing TUI start");
                    json!({"turn":{"id":"turn-voice"}})
                }
                method => panic!("unexpected lifecycle test method: {method}"),
            };
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("response");
        }
    });
    let session = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
    )
    .await
    .expect("connect test Analyst");
    (session, server)
}

async fn test_session_with_retryable_interrupt()
-> (AnalystSession, JoinHandle<()>, oneshot::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let (second_interrupt_tx, second_interrupt_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        let mut interrupts = 0_u8;
        let mut second_interrupt_tx = Some(second_interrupt_tx);
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.expect("frame") else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).expect("request");
            let Some(id) = request["id"].as_u64() else {
                continue;
            };
            let method = request["method"].as_str().expect("method");
            if method == "turn/interrupt" {
                interrupts += 1;
                if interrupts == 1 {
                    socket
                        .send(Message::Text(
                            json!({
                                "jsonrpc":"2.0",
                                "id":id,
                                "error":{"code":-32000,"message":"interrupt unavailable"}
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .expect("interrupt error response");
                    continue;
                }
                second_interrupt_tx
                    .take()
                    .expect("single second interrupt")
                    .send(())
                    .expect("signal second interrupt");
            }
            let result = match method {
                "initialize" => json!({}),
                "thread/start" => json!({"thread":{"id":"thread-lifecycle"}}),
                "turn/interrupt" => json!({}),
                method => panic!("unexpected lifecycle test method: {method}"),
            };
            socket
                .send(Message::Text(
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("response");
        }
    });
    let session = AnalystSession::connect_for_test(
        WorkspaceBinding {
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
    )
    .await
    .expect("connect test Analyst");
    (session, server, second_interrupt_rx)
}
