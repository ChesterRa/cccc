use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite};
use tungstenite::Message;

pub(super) async fn fake_app_server() -> (
    String,
    JoinHandle<()>,
    Arc<AtomicUsize>,
    Arc<Mutex<Option<Value>>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let turn_starts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&turn_starts);
    let elicitation_response = Arc::new(Mutex::new(None));
    let captured_response = Arc::clone(&elicitation_response);
    let server = tokio::spawn(async move {
        for connection in 0..2 {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = accept_async(stream).await.expect("websocket");
            let mut materialized = connection > 0;
            while let Some(frame) = socket.next().await {
                let frame = match frame {
                    Ok(frame) => frame,
                    Err(error) if is_peer_shutdown(&error) => break,
                    Err(error) => panic!("frame: {error}"),
                };
                let Message::Text(text) = frame else {
                    continue;
                };
                let request: Value = serde_json::from_str(&text).expect("request");
                if request.get("method").is_none() {
                    *captured_response.lock().expect("response lock") = Some(request);
                    continue;
                }
                let id = request["id"].as_u64().expect("request id");
                match request["method"].as_str().expect("method") {
                    "initialize" => send_result(&mut socket, id, json!({})).await,
                    "thread/start" => {
                        assert_eq!(connection, 0);
                        assert_eq!(
                            request["params"]["cwd"],
                            json!(std::env::current_dir().expect("cwd"))
                        );
                        assert!(request["params"].get("model").is_none());
                        assert_eq!(request["params"]["approvalPolicy"], "never");
                        assert_eq!(request["params"]["sandbox"], "danger-full-access");
                        assert_eq!(request["params"]["historyMode"], "legacy");
                        assert!(
                            request["params"]["developerInstructions"]
                                .as_str()
                                .is_some_and(|text| {
                                    text.contains("CCCC Realtime Voice")
                                        && text.contains("explicit group_id")
                                })
                        );
                        send_result(&mut socket, id, json!({"thread":{"id":"thread-1"}})).await;
                    }
                    "thread/name/set" => {
                        assert_eq!(connection, 0, "resumed threads must not be renamed");
                        assert_eq!(request["params"]["threadId"], "thread-1");
                        assert_eq!(request["params"]["name"], "CCCC Voice Analyst");
                        materialized = true;
                        send_result(&mut socket, id, json!({})).await;
                    }
                    "thread/read" => {
                        assert!(materialized, "a reserved thread id is not durable history");
                        assert_eq!(request["params"]["includeTurns"], true);
                        send_result(
                            &mut socket,
                            id,
                            json!({"thread":{"id":"thread-1","turns":[]}}),
                        )
                        .await;
                    }
                    "thread/resume" => {
                        assert_eq!(connection, 1);
                        assert_eq!(request["params"]["threadId"], "thread-1");
                        assert_eq!(
                            request["params"]["cwd"],
                            json!(std::env::current_dir().expect("cwd"))
                        );
                        assert_eq!(request["params"]["approvalPolicy"], "never");
                        assert_eq!(request["params"]["sandbox"], "danger-full-access");
                        assert!(request["params"].get("historyMode").is_none());
                        send_result(&mut socket, id, json!({"thread":{"id":"thread-1"}})).await;
                    }
                    "turn/start" => {
                        assert!(materialized);
                        let turn_number = counter.fetch_add(1, Ordering::SeqCst) + 1;
                        let turn_id = format!("turn-{turn_number}");
                        assert_eq!(request["params"]["threadId"], "thread-1");
                        assert!(
                            request["params"]["responsesapiClientMetadata"]
                                .get("cccc_group_id")
                                .is_none()
                        );
                        send_result(&mut socket, id, json!({"turn":{"id":turn_id}})).await;
                        socket
                            .send(Message::Text(
                                json!({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":turn_id,"status":"completed"}}})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .expect("completion");
                    }
                    "turn/steer" => {
                        assert_eq!(request["params"]["expectedTurnId"], "turn-1");
                        send_result(&mut socket, id, json!({"turnId":"turn-1"})).await;
                    }
                    "turn/interrupt" => {
                        assert_eq!(request["params"]["turnId"], "turn-1");
                        send_result(&mut socket, id, json!({})).await;
                        socket
                            .send(Message::Text(
                                json!({
                                    "jsonrpc":"2.0", "id":"elicitation-1",
                                    "method":"mcpServer/elicitation/request",
                                    "params":{"serverName":"cccc","threadId":"thread-1",
                                        "turnId":"turn-1","mode":"form","message":"Allow?",
                                        "requestedSchema":{"type":"object","properties":{}}}
                                })
                                .to_string()
                                .into(),
                            ))
                            .await
                            .expect("elicitation");
                    }
                    method => panic!("unexpected method: {method}"),
                }
            }
        }
    });
    (endpoint, server, turn_starts, elicitation_response)
}

pub(super) async fn fake_disconnecting_app_server() -> (String, JoinHandle<()>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let turn_starts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&turn_starts);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        while let Some(frame) = socket.next().await {
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) if is_peer_shutdown(&error) => break,
                Err(error) => panic!("frame: {error}"),
            };
            let Message::Text(text) = frame else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).expect("request");
            let id = request["id"].as_u64().expect("request id");
            match request["method"].as_str().expect("method") {
                "initialize" => send_result(&mut socket, id, json!({})).await,
                "thread/start" => {
                    send_result(&mut socket, id, json!({"thread":{"id":"thread-drop"}})).await;
                }
                "thread/name/set" => send_result(&mut socket, id, json!({})).await,
                "thread/read" => {
                    send_result(
                        &mut socket,
                        id,
                        json!({"thread":{"id":"thread-drop","turns":[]}}),
                    )
                    .await;
                }
                "turn/start" => {
                    counter.fetch_add(1, Ordering::SeqCst);
                    socket.close(None).await.expect("close websocket");
                    return;
                }
                method => panic!("unexpected method: {method}"),
            }
        }
    });
    (endpoint, server, turn_starts)
}

fn is_peer_shutdown(error: &tungstenite::Error) -> bool {
    matches!(
        error,
        tungstenite::Error::Protocol(
            tungstenite::error::ProtocolError::ResetWithoutClosingHandshake
        )
    ) || matches!(
        error,
        tungstenite::Error::Io(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
            )
    )
}

async fn send_result<S>(socket: &mut WebSocketStream<S>, id: u64, result: Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"result":result})
                .to_string()
                .into(),
        ))
        .await
        .expect("response");
}
