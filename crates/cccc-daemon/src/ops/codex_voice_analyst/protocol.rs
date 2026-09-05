use super::AnalystEvent;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
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

struct PendingResponse {
    response: oneshot::Sender<io::Result<Value>>,
    turn_delegation_id: Option<String>,
}

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
                tracing::debug!(%error, "waiting for Codex app-server websocket");
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
    RespondError { id: Value, error: Value },
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
        let work = self.request_inner(method, params, timeout);
        match super::lifecycle_timing::request_phase(method) {
            Some(phase) => super::lifecycle_timing::run(phase, work).await,
            None => work.await,
        }
    }

    async fn request_inner(
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

    pub(super) async fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        self.commands
            .send(ProtocolCommand::RespondError { id, error })
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
    let mut pending: HashMap<u64, PendingResponse> = HashMap::new();
    let mut pending_turn_start = None;
    let mut deferred_events = VecDeque::new();
    let mut turn_delegations = HashMap::new();
    let terminal_error = loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(ProtocolCommand::Request(request)) => {
                    let id = next_id;
                    next_id = next_id.saturating_add(1);
                    let turn_delegation_id = (request.method == "turn/start")
                        .then(|| {
                            let metadata = request.params.get("responsesapiClientMetadata")?;
                            metadata
                                .get(super::CODEX_TURN_CORRELATION_KEY)
                                .or_else(|| metadata.get("cccc_voice_delegation_id"))?
                                .as_str()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_owned)
                        })
                        .flatten();
                    let message = json!({
                        "jsonrpc":"2.0", "id":id,
                        "method":request.method, "params":request.params,
                    });
                    if turn_delegation_id.is_some() {
                        if pending_turn_start.is_some() {
                            let _ = request.response.send(Err(io::Error::new(
                                io::ErrorKind::WouldBlock,
                                "another correlated Codex turn/start request is unresolved",
                            )));
                            continue;
                        }
                        pending_turn_start = Some(id);
                    }
                    pending.insert(id, PendingResponse {
                        response: request.response,
                        turn_delegation_id,
                    });
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
                Some(ProtocolCommand::RespondError { id, error }) => {
                    let message = json!({"jsonrpc":"2.0","id":id,"error":error});
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
                        if pending_turn_start.is_some() {
                            if deferred_events.len() >= EVENT_CAPACITY {
                                break "Codex app-server produced too many events before turn/start settled".into();
                            }
                            deferred_events.push_back(message);
                        } else {
                            publish_event(
                                &events,
                                &generation,
                                message,
                                &mut turn_delegations,
                            );
                        }
                        continue;
                    }
                    let Some(id) = message.get("id").and_then(Value::as_u64) else { continue };
                    let Some(pending_response) = pending.remove(&id) else { continue };
                    let result = if let Some(error) = message.get("error") {
                        Err(io::Error::other(format!(
                            "Codex app-server request failed: {error}"
                        )))
                    } else {
                        Ok(message.get("result").cloned().unwrap_or(Value::Null))
                    };
                    if pending_turn_start == Some(id) {
                        pending_turn_start = None;
                        let response_turn_id = result
                            .as_ref()
                            .ok()
                            .and_then(|result| result
                                .get("turn")
                                .and_then(|turn| turn.get("id"))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty()));
                        if let Some(response_turn_id) = response_turn_id
                            && deferred_events.iter().any(|event| {
                                started_turn_id(event)
                                    .is_some_and(|turn_id| turn_id != response_turn_id)
                            })
                        {
                            break "a competing terminal turn started while a correlated Codex request was pending".into();
                        }
                        if let (Some(delegation_id), Some(turn_id)) =
                            (pending_response.turn_delegation_id.as_ref(), response_turn_id)
                        {
                            turn_delegations.insert(turn_id.to_owned(), delegation_id.clone());
                        }
                        while let Some(event) = deferred_events.pop_front() {
                            publish_event(
                                &events,
                                &generation,
                                event,
                                &mut turn_delegations,
                            );
                        }
                    }
                    let _ = pending_response.response.send(result);
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
    for (_, pending_response) in pending {
        let _ = pending_response.response.send(Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            terminal_error.clone(),
        )));
    }
    let _ = events.send(AnalystEvent {
        generation,
        message: json!({"method":super::MANAGED_AGENT_DISCONNECTED_METHOD,"params":{"reason":terminal_error}}),
        requested_delegation_id: None,
    });
}

fn started_turn_id(message: &Value) -> Option<&str> {
    (message.get("method").and_then(Value::as_str) == Some("turn/started"))
        .then(|| {
            message
                .get("params")?
                .get("turn")?
                .get("id")?
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .flatten()
}

fn publish_event(
    events: &broadcast::Sender<AnalystEvent>,
    generation: &str,
    message: Value,
    turn_delegations: &mut HashMap<String, String>,
) {
    let method = message.get("method").and_then(Value::as_str);
    let turn_id = message
        .get("params")
        .and_then(|params| params.get("turn"))
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_delegation_id = if method == Some("turn/started") {
        turn_id.and_then(|turn_id| turn_delegations.remove(turn_id))
    } else {
        if method == Some("turn/completed")
            && let Some(turn_id) = turn_id
        {
            turn_delegations.remove(turn_id);
        }
        None
    };
    let _ = events.send(AnalystEvent {
        generation: generation.to_owned(),
        message,
        requested_delegation_id,
    });
}
