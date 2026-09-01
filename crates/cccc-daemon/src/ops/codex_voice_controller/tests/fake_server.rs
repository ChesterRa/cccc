use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

pub(super) async fn fake_analyst_server()
-> (String, JoinHandle<()>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
    let starts = Arc::new(AtomicUsize::new(0));
    let start_count = Arc::clone(&starts);
    let steers = Arc::new(AtomicUsize::new(0));
    let steer_count = Arc::clone(&steers);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket");
        let mut active_turn = String::new();
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.expect("frame") else {
                continue;
            };
            let request: Value = serde_json::from_str(&text).expect("request");
            if request.get("method").is_none() {
                assert_eq!(request["result"]["action"], "decline");
                continue;
            }
            let id = request["id"].as_u64().expect("id");
            match request["method"].as_str().expect("method") {
                "initialize" => send_result(&mut socket, id, json!({})).await,
                "thread/start" => {
                    send_result(
                        &mut socket,
                        id,
                        json!({"thread":{"id":"thread-controller"}}),
                    )
                    .await;
                }
                "turn/start" => {
                    let count = start_count.fetch_add(1, Ordering::SeqCst) + 1;
                    active_turn = format!("turn-controller-{count}");
                    socket
                        .send(Message::Text(
                            json!({
                                "method":"turn/started",
                                "params":{"threadId":"thread-controller","turn":{"id":active_turn}}
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .expect("turn started notification");
                    send_result(&mut socket, id, json!({"turn":{"id":active_turn}})).await;
                }
                "turn/steer" => {
                    assert_eq!(request["params"]["expectedTurnId"], active_turn);
                    let count = steer_count.fetch_add(1, Ordering::SeqCst) + 1;
                    send_result(&mut socket, id, json!({})).await;
                    if count == 2 {
                        send_terminal(&mut socket, &active_turn, "completed").await;
                    }
                }
                "turn/interrupt" => {
                    assert_eq!(request["params"]["turnId"], active_turn);
                    send_result(&mut socket, id, json!({})).await;
                    socket
                        .send(Message::Text(
                            json!({
                                "jsonrpc":"2.0", "id":"elicitation-controller",
                                "method":"mcpServer/elicitation/request",
                                "params":{"threadId":"thread-controller","turnId":active_turn}
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .expect("elicitation");
                    send_terminal(&mut socket, &active_turn, "interrupted").await;
                }
                method => panic!("unexpected method: {method}"),
            }
        }
    });
    (endpoint, server, starts, steers)
}

pub(super) async fn fake_disconnecting_analyst_server() -> (String, JoinHandle<()>) {
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
            let id = request["id"].as_u64().expect("id");
            match request["method"].as_str().expect("method") {
                "initialize" => send_result(&mut socket, id, json!({})).await,
                "thread/start" => {
                    send_result(
                        &mut socket,
                        id,
                        json!({"thread":{"id":"thread-disconnect"}}),
                    )
                    .await;
                }
                "turn/start" => {
                    socket.close(None).await.expect("close websocket");
                    return;
                }
                method => panic!("unexpected disconnect method: {method}"),
            }
        }
    });
    (endpoint, server)
}

async fn send_terminal<S>(socket: &mut WebSocketStream<S>, turn_id: &str, status: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(
            json!({
                "method":"turn/completed",
                "params":{"threadId":"thread-controller","turn":{"id":turn_id,"status":status}}
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("terminal notification");
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
