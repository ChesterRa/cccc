use super::events::{tracked_work, truncate_utf8};
use super::*;
use crate::ops::codex_voice_analyst::ScopeBinding;
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

    release_response.send(()).expect("release start response");
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
                "event":{"group_id":"g_target"}
            }}}
        }}
    }))
    .expect("tracked work");
    assert_eq!(work.task_id, "T007");
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
        ScopeBinding {
            group_id: "g_lifecycle".into(),
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
        None,
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
        ScopeBinding {
            group_id: "g_lifecycle".into(),
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
        None,
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
        ScopeBinding {
            group_id: "g_lifecycle".into(),
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
        None,
    )
    .await
    .expect("connect test Analyst");
    (session, server, release_response_tx)
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
        ScopeBinding {
            group_id: "g_lifecycle".into(),
            root: PathBuf::from("/tmp"),
        },
        "generation-lifecycle".into(),
        endpoint,
        PathBuf::from("codex"),
        None,
    )
    .await
    .expect("connect test Analyst");
    (session, server, second_interrupt_rx)
}
