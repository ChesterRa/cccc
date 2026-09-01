use super::AnalystEvent;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
const EVENT_CAPACITY: usize = 2048;
const COMMAND_CAPACITY: usize = 32;

pub(super) async fn connect_with_retry(
    endpoint: &str,
) -> io::Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        match tokio_tungstenite::connect_async(endpoint).await {
            Ok((socket, _)) => return Ok(socket),
            Err(error) if tokio::time::Instant::now() < deadline => {
                tracing::debug!(%error, "waiting for Voice Analyst app-server websocket");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("could not connect to Codex app-server: {error}"),
                ));
            }
        }
    }
}

struct RpcRequest {
    method: String,
    params: Value,
    response: oneshot::Sender<io::Result<Value>>,
}

enum ProtocolCommand {
    Request(RpcRequest),
    Respond { id: Value, result: Value },
    Close,
}

pub(super) struct ProtocolClient {
    commands: mpsc::Sender<ProtocolCommand>,
    pub(super) events: broadcast::Sender<AnalystEvent>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl ProtocolClient {
    pub(super) fn new<S>(socket: tokio_tungstenite::WebSocketStream<S>, generation: String) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (commands, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let task = tokio::spawn(protocol_loop(socket, receiver, events.clone(), generation));
        Self {
            commands,
            events,
            task: Mutex::new(Some(task)),
        }
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<AnalystEvent> {
        self.events.subscribe()
    }

    pub(super) async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> io::Result<Value> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(ProtocolCommand::Request(RpcRequest {
                method: method.into(),
                params,
                response: sender,
            }))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "app-server is closed"))?;
        tokio::time::timeout(timeout, receiver)
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("Codex app-server request timed out: {method}"),
                )
            })?
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "app-server is closed"))?
    }

    pub(super) async fn respond(&self, id: Value, result: Value) -> io::Result<()> {
        self.commands
            .send(ProtocolCommand::Respond { id, result })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "app-server is closed"))
    }

    pub(super) async fn close(&self) {
        let _ = self.commands.send(ProtocolCommand::Close).await;
        let task = self.task.lock().ok().and_then(|mut task| task.take());
        if let Some(mut task) = task
            && tokio::time::timeout(STOP_TIMEOUT, &mut task).await.is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for ProtocolClient {
    fn drop(&mut self) {
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

async fn protocol_loop<S>(
    mut socket: tokio_tungstenite::WebSocketStream<S>,
    mut commands: mpsc::Receiver<ProtocolCommand>,
    events: broadcast::Sender<AnalystEvent>,
    generation: String,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut next_id = 1_u64;
    let mut pending: HashMap<u64, oneshot::Sender<io::Result<Value>>> = HashMap::new();
    let terminal_error = loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(ProtocolCommand::Request(request)) => {
                    let id = next_id;
                    next_id = next_id.saturating_add(1);
                    let message = json!({
                        "jsonrpc":"2.0", "id":id,
                        "method":request.method, "params":request.params,
                    });
                    pending.insert(id, request.response);
                    if let Err(error) = socket.send(Message::Text(message.to_string().into())).await {
                        break format!("failed to write app-server request: {error}");
                    }
                }
                Some(ProtocolCommand::Respond { id, result }) => {
                    let message = json!({"jsonrpc":"2.0","id":id,"result":result});
                    if let Err(error) = socket.send(Message::Text(message.to_string().into())).await {
                        break format!("failed to write app-server response: {error}");
                    }
                }
                Some(ProtocolCommand::Close) | None => {
                    let _ = socket.close(None).await;
                    break "app-server client closed".to_owned();
                }
            },
            frame = socket.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                    if message.get("method").is_some() {
                        let _ = events.send(AnalystEvent {
                            generation: generation.clone(), message,
                        });
                        continue;
                    }
                    let Some(id) = message.get("id").and_then(Value::as_u64) else { continue };
                    let Some(response) = pending.remove(&id) else { continue };
                    if let Some(error) = message.get("error") {
                        let _ = response.send(Err(io::Error::other(format!(
                            "Codex app-server request failed: {error}"
                        ))));
                    } else {
                        let _ = response.send(Ok(message.get("result").cloned().unwrap_or(Value::Null)));
                    }
                }
                Some(Ok(Message::Ping(payload))) => {
                    if let Err(error) = socket.send(Message::Pong(payload)).await {
                        break format!("failed to answer app-server ping: {error}");
                    }
                }
                Some(Ok(Message::Close(_))) | None => break "app-server websocket closed".to_owned(),
                Some(Ok(_)) => {}
                Some(Err(error)) => break format!("app-server websocket failed: {error}"),
            }
        }
    };
    for (_, response) in pending {
        let _ = response.send(Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            terminal_error.clone(),
        )));
    }
    let _ = events.send(AnalystEvent {
        generation,
        message: json!({"method":"cccc/voiceAnalyst/disconnected","params":{"reason":terminal_error}}),
    });
}
