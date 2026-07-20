use futures_util::{SinkExt, StreamExt, future::BoxFuture};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_WS_URL: &str = "wss://openws.work.weixin.qq.com";
const SUBSCRIBE: &str = "aibot_subscribe";
const HEARTBEAT: &str = "ping";
const SEND_MSG: &str = "aibot_send_msg";
pub(super) const RESPOND_MSG: &str = "aibot_respond_msg";
#[allow(dead_code)]
pub(super) const RESPOND_WELCOME: &str = "aibot_respond_welcome_msg";
#[allow(dead_code)]
pub(super) const RESPOND_UPDATE: &str = "aibot_respond_update_msg";
pub(super) const UPLOAD_MEDIA_INIT: &str = "aibot_upload_media_init";
pub(super) const UPLOAD_MEDIA_CHUNK: &str = "aibot_upload_media_chunk";
pub(super) const UPLOAD_MEDIA_FINISH: &str = "aibot_upload_media_finish";
const MESSAGE_CALLBACK: &str = "aibot_msg_callback";
const EVENT_CALLBACK: &str = "aibot_event_callback";

type MessageHandler = Arc<dyn Fn(Value) -> BoxFuture<'static, ()> + Send + Sync>;
type StatusHandler = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone, Debug)]
enum ClientState {
    Connecting,
    Authenticated,
    Failed(String),
    Stopped,
}

struct SendCommand {
    frame: Value,
    req_id: String,
    deadline: tokio::time::Instant,
    result: oneshot::Sender<Result<Value, String>>,
}

pub(super) struct WecomClient {
    commands: mpsc::Sender<SendCommand>,
    shutdown: watch::Sender<bool>,
    authenticated: Arc<AtomicBool>,
    reply_timeout: Duration,
    reply_refs: Arc<Mutex<ReplyRefs>>,
}

#[derive(Clone)]
struct ClientOptions {
    url: String,
    heartbeat_interval: Duration,
    reconnect_base_delay: Duration,
    connect_timeout: Duration,
    reply_timeout: Duration,
}

#[derive(Default)]
struct ReplyRefs {
    values: HashMap<String, String>,
    order: VecDeque<String>,
}

struct ClientRuntime {
    bot_id: String,
    secret: String,
    handler: MessageHandler,
    options: ClientOptions,
    state: watch::Sender<ClientState>,
    authenticated: Arc<AtomicBool>,
    reply_refs: Arc<Mutex<ReplyRefs>>,
    on_terminal_error: StatusHandler,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            url: DEFAULT_WS_URL.into(),
            heartbeat_interval: Duration::from_secs(30),
            reconnect_base_delay: Duration::from_secs(1),
            connect_timeout: Duration::from_secs(10),
            reply_timeout: Duration::from_secs(5),
        }
    }
}

impl WecomClient {
    pub(super) async fn connect_with_status<H, Fut>(
        bot_id: String,
        secret: String,
        handler: H,
        on_terminal_error: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<(Arc<Self>, JoinHandle<()>), String>
    where
        H: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self::connect_configured(
            bot_id,
            secret,
            handler,
            on_terminal_error,
            ClientOptions::default(),
        )
        .await
    }

    #[cfg(test)]
    async fn connect_with_options<H, Fut>(
        bot_id: String,
        secret: String,
        handler: H,
        options: ClientOptions,
    ) -> Result<(Arc<Self>, JoinHandle<()>), String>
    where
        H: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self::connect_configured(bot_id, secret, handler, |_| {}, options).await
    }

    async fn connect_configured<H, Fut>(
        bot_id: String,
        secret: String,
        handler: H,
        on_terminal_error: impl Fn(String) + Send + Sync + 'static,
        options: ClientOptions,
    ) -> Result<(Arc<Self>, JoinHandle<()>), String>
    where
        H: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (command_tx, command_rx) = mpsc::channel(128);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (state_tx, mut state_rx) = watch::channel(ClientState::Connecting);
        let authenticated = Arc::new(AtomicBool::new(false));
        let reply_refs = Arc::new(Mutex::new(ReplyRefs::default()));
        let client = Arc::new(Self {
            commands: command_tx,
            shutdown: shutdown_tx,
            authenticated: Arc::clone(&authenticated),
            reply_timeout: options.reply_timeout,
            reply_refs: Arc::clone(&reply_refs),
        });
        let runtime = ClientRuntime {
            bot_id,
            secret,
            handler: Arc::new(move |frame| Box::pin(handler(frame))),
            options: options.clone(),
            state: state_tx,
            authenticated,
            reply_refs,
            on_terminal_error: Arc::new(on_terminal_error),
        };
        let task = tokio::spawn(run_client(runtime, command_rx, shutdown_rx));

        let initial = tokio::time::timeout(options.connect_timeout, async {
            loop {
                match state_rx.borrow().clone() {
                    ClientState::Authenticated => return Ok(()),
                    ClientState::Failed(error) => return Err(error),
                    ClientState::Stopped => return Err("WeCom connection stopped".into()),
                    ClientState::Connecting => {}
                }
                state_rx
                    .changed()
                    .await
                    .map_err(|_| "WeCom connection task ended before authentication".to_owned())?;
            }
        })
        .await;

        match initial {
            Ok(Ok(())) => Ok((client, task)),
            Ok(Err(error)) => {
                client.shutdown();
                task.abort();
                let _ = task.await;
                Err(error)
            }
            Err(_) => {
                client.shutdown();
                task.abort();
                let _ = task.await;
                Err(format!(
                    "WeCom authentication timed out after {:?}",
                    options.connect_timeout
                ))
            }
        }
    }

    pub(super) async fn send_message(&self, chat_id: &str, body: Value) -> Result<(), String> {
        let mut body = body;
        let Some(object) = body.as_object_mut() else {
            return Err("WeCom message body must be an object".into());
        };
        object.insert("chatid".into(), Value::String(chat_id.to_owned()));
        self.send_command(SEND_MSG, None, body, self.reply_timeout)
            .await
            .map(|_| ())
    }

    pub(super) async fn reply_message(&self, req_id: &str, body: Value) -> Result<Value, String> {
        self.reply_command(RESPOND_MSG, req_id, body).await
    }

    #[allow(dead_code)]
    pub(super) async fn reply_welcome(&self, req_id: &str, body: Value) -> Result<Value, String> {
        self.reply_command(RESPOND_WELCOME, req_id, body).await
    }

    #[allow(dead_code)]
    pub(super) async fn update_template_card(
        &self,
        req_id: &str,
        template_card: Value,
        user_ids: &[String],
    ) -> Result<Value, String> {
        let mut body = json!({
            "response_type":"update_template_card",
            "template_card":template_card
        });
        if !user_ids.is_empty() {
            body["userids"] = json!(user_ids);
        }
        self.reply_command(RESPOND_UPDATE, req_id, body).await
    }

    async fn reply_command(&self, cmd: &str, req_id: &str, body: Value) -> Result<Value, String> {
        if req_id.trim().is_empty() {
            return Err("WeCom callback reply req_id is empty".into());
        }
        self.send_command(cmd, Some(req_id.to_owned()), body, self.reply_timeout)
            .await
    }

    pub(super) fn reply_req_id(&self, chat_id: &str) -> Option<String> {
        self.reply_refs
            .lock()
            .expect("WeCom reply reference registry poisoned")
            .values
            .get(chat_id)
            .cloned()
    }

    pub(super) async fn send_command(
        &self,
        cmd: &str,
        req_id: Option<String>,
        body: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        if !self.authenticated.load(Ordering::Acquire) {
            return Err("WeCom WebSocket is not authenticated".into());
        }
        let req_id = req_id.unwrap_or_else(|| request_id(cmd));
        let frame = json!({"cmd":cmd,"headers":{"req_id":req_id.clone()},"body":body});
        let (result_tx, result_rx) = oneshot::channel();
        let deadline = tokio::time::Instant::now() + timeout;
        tokio::time::timeout_at(
            deadline,
            self.commands.send(SendCommand {
                frame,
                req_id,
                deadline,
                result: result_tx,
            }),
        )
        .await
        .map_err(|_| "WeCom command queue timed out".to_owned())?
        .map_err(|_| "WeCom connection task is not running".to_owned())?;
        tokio::time::timeout_at(deadline, result_rx)
            .await
            .map_err(|_| "WeCom command acknowledgement timed out".to_owned())?
            .map_err(|_| "WeCom command was cancelled".to_owned())?
    }

    pub(super) fn shutdown(&self) {
        self.authenticated.store(false, Ordering::Release);
        let _ = self.shutdown.send(true);
    }
}

async fn run_client(
    runtime: ClientRuntime,
    mut commands: mpsc::Receiver<SendCommand>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut reconnect_attempt = 0_u32;
    let mut auth_failures = 0_u8;
    let mut ever_authenticated = false;
    let mut terminal_error = None;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let result = run_session(&runtime, &mut commands, &mut shutdown).await;
        let was_authenticated = runtime.authenticated.swap(false, Ordering::AcqRel);
        if was_authenticated {
            ever_authenticated = true;
            auth_failures = 0;
        }
        match result {
            SessionEnd::Stopped => break,
            SessionEnd::Replaced => {
                terminal_error = Some("WeCom connection was replaced by a new connection".into());
                break;
            }
            SessionEnd::AuthenticationFailed(error) => {
                auth_failures = auth_failures.saturating_add(1);
                if !ever_authenticated || auth_failures >= 5 {
                    (runtime.on_terminal_error)(error.clone());
                    let _ = runtime.state.send(ClientState::Failed(error));
                    reject_queued(&mut commands, "WeCom authentication failed");
                    return;
                }
                tracing::warn!(
                    %error,
                    attempt = auth_failures,
                    "WeCom reauthentication failed; retrying"
                );
                let _ = runtime.state.send(ClientState::Connecting);
            }
            SessionEnd::Disconnected(error) => {
                tracing::warn!(%error, "WeCom WebSocket disconnected");
                let _ = runtime.state.send(ClientState::Connecting);
            }
        }

        reconnect_attempt = if was_authenticated {
            1
        } else {
            reconnect_attempt.saturating_add(1)
        };
        let multiplier = 1_u32 << reconnect_attempt.saturating_sub(1).min(5);
        let delay =
            (runtime.options.reconnect_base_delay * multiplier).min(Duration::from_secs(30));
        tracing::info!(
            attempt = reconnect_attempt,
            ?delay,
            "reconnecting WeCom WebSocket"
        );
        if wait_backoff(delay, &mut commands, &mut shutdown).await {
            break;
        }
    }
    if let Some(error) = terminal_error {
        (runtime.on_terminal_error)(error);
    }
    runtime.authenticated.store(false, Ordering::Release);
    reject_queued(&mut commands, "WeCom connection stopped");
    let _ = runtime.state.send(ClientState::Stopped);
}

enum SessionEnd {
    Stopped,
    Replaced,
    AuthenticationFailed(String),
    Disconnected(String),
}

async fn run_session(
    runtime: &ClientRuntime,
    commands: &mut mpsc::Receiver<SendCommand>,
    shutdown: &mut watch::Receiver<bool>,
) -> SessionEnd {
    let (socket, _) = match tokio_tungstenite::connect_async(&runtime.options.url).await {
        Ok(socket) => socket,
        Err(error) => return SessionEnd::Disconnected(format!("connection failed: {error}")),
    };
    let (mut writer, mut reader) = socket.split();
    let subscribe_id = request_id(SUBSCRIBE);
    let subscribe = json!({
        "cmd": SUBSCRIBE,
        "headers": {"req_id": subscribe_id},
        "body": {"bot_id": runtime.bot_id, "secret": runtime.secret}
    });
    if let Err(error) = writer.send(text_message(&subscribe)).await {
        return SessionEnd::Disconnected(format!("failed to send subscription: {error}"));
    }
    tracing::info!("WeCom subscription frame sent");

    let auth_result = tokio::time::timeout(runtime.options.connect_timeout, async {
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Err(SessionEnd::Stopped);
                    }
                }
                message = reader.next() => {
                    let frame = match parse_frame(message) {
                        Ok(Some(frame)) => frame,
                        Ok(None) => continue,
                        Err(end) => return Err(end),
                    };
                    if frame.pointer("/headers/req_id").and_then(Value::as_str) == Some(&subscribe_id) {
                        let code = frame.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
                        if code == 0 {
                            return Ok(());
                        }
                        let message = frame.get("errmsg").and_then(Value::as_str).unwrap_or("unknown error");
                        return Err(SessionEnd::AuthenticationFailed(format!(
                            "WeCom authentication failed: {message} (code: {code})"
                        )));
                    }
                }
            }
        }
    }).await;
    match auth_result {
        Ok(Ok(())) => {}
        Ok(Err(end)) => return end,
        Err(_) => return SessionEnd::Disconnected("subscription acknowledgement timed out".into()),
    }

    runtime.authenticated.store(true, Ordering::Release);
    let _ = runtime.state.send(ClientState::Authenticated);
    tracing::info!("WeCom authentication successful");
    let mut heartbeat = tokio::time::interval(runtime.options.heartbeat_interval);
    heartbeat.tick().await;
    let mut missed_heartbeats = 0_u8;
    let mut pending: HashMap<String, PendingSend> = HashMap::new();
    let mut reply_expiry = tokio::time::interval(Duration::from_millis(250));
    reply_expiry.tick().await;

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    reject_pending(&mut pending, "WeCom connection stopped");
                    let _ = writer.close().await;
                    return SessionEnd::Stopped;
                }
            }
            Some(command) = commands.recv() => {
                if command.result.is_closed() || command.deadline <= tokio::time::Instant::now() {
                    continue;
                }
                if pending.contains_key(&command.req_id) {
                    let _ = command.result.send(Err(format!(
                        "WeCom command already pending for req_id {}", command.req_id
                    )));
                    continue;
                }
                if let Err(error) = writer.send(text_message(&command.frame)).await {
                    let message = format!("failed to send WeCom message: {error}");
                    let _ = command.result.send(Err(message.clone()));
                    reject_pending(&mut pending, &message);
                    return SessionEnd::Disconnected(message);
                }
                pending.insert(command.req_id, PendingSend {
                    result: command.result,
                    expires_at: command.deadline,
                });
            }
            _ = reply_expiry.tick() => expire_pending(&mut pending),
            _ = heartbeat.tick() => {
                if missed_heartbeats >= 2 {
                    reject_pending(&mut pending, "WeCom heartbeat acknowledgement timed out");
                    return SessionEnd::Disconnected("heartbeat acknowledgement timed out".into());
                }
                missed_heartbeats += 1;
                let frame = json!({"cmd": HEARTBEAT, "headers": {"req_id": request_id(HEARTBEAT)}});
                if let Err(error) = writer.send(text_message(&frame)).await {
                    reject_pending(&mut pending, "WeCom heartbeat send failed");
                    return SessionEnd::Disconnected(format!("heartbeat send failed: {error}"));
                }
            }
            message = reader.next() => {
                let frame = match parse_frame(message) {
                    Ok(Some(frame)) => frame,
                    Ok(None) => continue,
                    Err(end) => {
                        reject_pending(&mut pending, "WeCom WebSocket disconnected");
                        return end;
                    }
                };
                let cmd = frame.get("cmd").and_then(Value::as_str).unwrap_or_default();
                if cmd == MESSAGE_CALLBACK || cmd == EVENT_CALLBACK {
                    if cmd == EVENT_CALLBACK
                        && frame.pointer("/body/event/eventtype").and_then(Value::as_str) == Some("disconnected_event")
                    {
                        (runtime.handler)(frame).await;
                        reject_pending(&mut pending, "WeCom connection was replaced by a new connection");
                        return SessionEnd::Replaced;
                    }
                    capture_reply_ref(runtime, &frame);
                    (runtime.handler)(frame).await;
                    continue;
                }
                let req_id = frame.pointer("/headers/req_id").and_then(Value::as_str).unwrap_or_default();
                if req_id.starts_with(HEARTBEAT) {
                    if frame.get("errcode").and_then(Value::as_i64) == Some(0) {
                        missed_heartbeats = 0;
                    }
                    continue;
                }
                if let Some(pending_send) = pending.remove(req_id) {
                    let code = frame.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
                    let reply = if code == 0 {
                        Ok(frame)
                    } else {
                        let message = frame.get("errmsg").and_then(Value::as_str).unwrap_or("unknown error");
                        Err(format!("WeCom send failed: {message} (code: {code})"))
                    };
                    let _ = pending_send.result.send(reply);
                }
            }
        }
    }
}

fn parse_frame(
    message: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Result<Option<Value>, SessionEnd> {
    match message {
        Some(Ok(Message::Text(text))) => Ok(parse_json_frame(text.as_bytes())),
        Some(Ok(Message::Binary(bytes))) => Ok(parse_json_frame(&bytes)),
        Some(Ok(Message::Close(frame))) => Err(SessionEnd::Disconnected(frame.map_or_else(
            || "connection closed".into(),
            |frame| frame.reason.to_string(),
        ))),
        Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => Ok(None),
        Some(Err(error)) => Err(SessionEnd::Disconnected(error.to_string())),
        None => Err(SessionEnd::Disconnected("connection ended".into())),
    }
}

fn parse_json_frame(bytes: &[u8]) -> Option<Value> {
    match serde_json::from_slice(bytes) {
        Ok(frame) => Some(frame),
        Err(error) => {
            tracing::warn!(%error, "ignored invalid WeCom JSON frame");
            None
        }
    }
}

fn capture_reply_ref(runtime: &ClientRuntime, frame: &Value) {
    let Some(req_id) = frame
        .pointer("/headers/req_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let Some(chat_id) = frame
        .pointer("/body/chatid")
        .and_then(Value::as_str)
        .or_else(|| frame.pointer("/body/from/userid").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let mut refs = runtime
        .reply_refs
        .lock()
        .expect("WeCom reply reference registry poisoned");
    refs.values.insert(chat_id.to_owned(), req_id.to_owned());
    refs.order.retain(|value| value != chat_id);
    refs.order.push_back(chat_id.to_owned());
    while refs.order.len() > 256 {
        if let Some(oldest) = refs.order.pop_front() {
            refs.values.remove(&oldest);
        }
    }
}

async fn wait_backoff(
    delay: Duration,
    commands: &mut mpsc::Receiver<SendCommand>,
    shutdown: &mut watch::Receiver<bool>,
) -> bool {
    let timer = tokio::time::sleep(delay);
    tokio::pin!(timer);
    loop {
        tokio::select! {
            _ = &mut timer => return false,
            changed = shutdown.changed() => return changed.is_err() || *shutdown.borrow(),
            Some(command) = commands.recv() => {
                if !command.result.is_closed() && command.deadline > tokio::time::Instant::now() {
                    let _ = command.result.send(Err("WeCom WebSocket is reconnecting".into()));
                }
            }
        }
    }
}

struct PendingSend {
    result: oneshot::Sender<Result<Value, String>>,
    expires_at: tokio::time::Instant,
}

fn reject_pending(pending: &mut HashMap<String, PendingSend>, reason: &str) {
    for (_, pending_send) in pending.drain() {
        let _ = pending_send.result.send(Err(reason.to_owned()));
    }
}

fn expire_pending(pending: &mut HashMap<String, PendingSend>) {
    let now = tokio::time::Instant::now();
    let expired: Vec<String> = pending
        .iter()
        .filter(|(_, pending_send)| pending_send.expires_at <= now)
        .map(|(req_id, _)| req_id.clone())
        .collect();
    for req_id in expired {
        if let Some(pending_send) = pending.remove(&req_id) {
            let _ = pending_send
                .result
                .send(Err("WeCom send acknowledgement timed out".into()));
        }
    }
}

fn reject_queued(commands: &mut mpsc::Receiver<SendCommand>, reason: &str) {
    while let Ok(command) = commands.try_recv() {
        let _ = command.result.send(Err(reason.to_owned()));
    }
}

fn request_id(prefix: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let random = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}_{timestamp}_{}", &random[..8])
}

fn text_message(value: &Value) -> Message {
    Message::Text(value.to_string().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn timed_out_queued_command_is_marked_cancelled_before_dispatch() {
        let (commands, mut receiver) = mpsc::channel(1);
        let (shutdown, _) = watch::channel(false);
        let client = WecomClient {
            commands,
            shutdown,
            authenticated: Arc::new(AtomicBool::new(true)),
            reply_timeout: Duration::from_millis(10),
            reply_refs: Arc::new(Mutex::new(ReplyRefs::default())),
        };

        let error = client
            .send_command("test", None, json!({}), Duration::from_millis(10))
            .await
            .expect_err("command without consumer must time out");
        assert!(error.contains("timed out"));
        let command = receiver.recv().await.expect("queued command");
        assert!(command.result.is_closed());
        assert!(command.deadline <= tokio::time::Instant::now());
    }

    #[tokio::test]
    async fn subscribes_before_heartbeat_and_handles_callbacks_and_active_sends() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("websocket");

            let subscribe = next_json(&mut socket).await;
            assert_eq!(subscribe["cmd"], SUBSCRIBE);
            assert_eq!(subscribe["body"]["bot_id"], "bot-id");
            assert_eq!(subscribe["body"]["secret"], "bot-secret");
            assert!(
                tokio::time::timeout(Duration::from_millis(60), socket.next())
                    .await
                    .is_err(),
                "heartbeat must not be sent before the subscription acknowledgement"
            );

            let subscribe_id = subscribe["headers"]["req_id"]
                .as_str()
                .expect("subscription req_id");
            socket
                .send(text_message(&json!({
                    "headers":{"req_id":subscribe_id},"errcode":0,"errmsg":"ok"
                })))
                .await
                .expect("subscription ack");
            socket
                .send(text_message(&json!({
                    "cmd":MESSAGE_CALLBACK,
                    "headers":{"req_id":"callback_1"},
                    "body":{"chatid":"chat-1","from":{"userid":"user-1"},"text":{"content":"hello"}}
                })))
                .await
                .expect("callback");

            let send = loop {
                let frame = next_json(&mut socket).await;
                if frame["cmd"] == HEARTBEAT {
                    acknowledge(&mut socket, &frame).await;
                } else {
                    break frame;
                }
            };
            assert_eq!(send["cmd"], SEND_MSG);
            assert_eq!(send["body"]["chatid"], "chat-1");
            assert_eq!(send["body"]["markdown"]["content"], "reply");
            acknowledge(&mut socket, &send).await;

            loop {
                let frame = next_json(&mut socket).await;
                if frame["cmd"] == HEARTBEAT {
                    acknowledge(&mut socket, &frame).await;
                    break;
                }
            }
            let _ = socket.close(None).await;
        });

        let (callback_tx, mut callback_rx) = mpsc::unbounded_channel();
        let options = ClientOptions {
            url: format!("ws://{address}"),
            heartbeat_interval: Duration::from_millis(100),
            reconnect_base_delay: Duration::from_millis(20),
            connect_timeout: Duration::from_secs(2),
            reply_timeout: Duration::from_secs(1),
        };
        let (client, task) = WecomClient::connect_with_options(
            "bot-id".into(),
            "bot-secret".into(),
            move |frame| {
                let callback_tx = callback_tx.clone();
                async move {
                    let _ = callback_tx.send(frame);
                }
            },
            options,
        )
        .await
        .expect("authenticated client");

        let callback = tokio::time::timeout(Duration::from_secs(1), callback_rx.recv())
            .await
            .expect("callback timeout")
            .expect("callback");
        assert_eq!(callback["body"]["from"]["userid"], "user-1");
        client
            .send_message(
                "chat-1",
                json!({"msgtype":"markdown","markdown":{"content":"reply"}}),
            )
            .await
            .expect("active send");

        server.await.expect("server task");
        client.shutdown();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("client stop timeout")
            .expect("client task");
    }

    #[tokio::test]
    async fn reports_subscription_authentication_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("websocket");
            let subscribe = next_json(&mut socket).await;
            let req_id = subscribe["headers"]["req_id"].as_str().expect("req_id");
            socket
                .send(text_message(&json!({
                    "headers":{"req_id":req_id},
                    "errcode":40001,
                    "errmsg":"invalid credential"
                })))
                .await
                .expect("auth error");
        });
        let options = ClientOptions {
            url: format!("ws://{address}"),
            connect_timeout: Duration::from_secs(1),
            ..ClientOptions::default()
        };
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();
        let result = WecomClient::connect_configured(
            "bad-bot".into(),
            "bad-secret".into(),
            |_| async {},
            move |error| {
                let _ = status_tx.send(error);
            },
            options,
        )
        .await;
        let error = match result {
            Ok(_) => panic!("authentication should fail"),
            Err(error) => error,
        };
        assert!(error.contains("invalid credential"));
        assert!(error.contains("40001"));
        assert!(
            status_rx
                .recv()
                .await
                .expect("terminal error")
                .contains("40001")
        );
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn active_send_times_out_when_ack_is_missing() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("websocket");
            let subscribe = next_json(&mut socket).await;
            acknowledge(&mut socket, &subscribe).await;
            let send = next_json(&mut socket).await;
            assert_eq!(send["cmd"], SEND_MSG);
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let options = ClientOptions {
            url: format!("ws://{address}"),
            heartbeat_interval: Duration::from_secs(1),
            reconnect_base_delay: Duration::from_millis(20),
            connect_timeout: Duration::from_secs(1),
            reply_timeout: Duration::from_millis(80),
        };
        let (client, task) = WecomClient::connect_with_options(
            "bot-id".into(),
            "bot-secret".into(),
            |_| async {},
            options,
        )
        .await
        .expect("client");
        let error = client
            .send_message(
                "chat",
                json!({"msgtype":"markdown","markdown":{"content":"x"}}),
            )
            .await
            .expect_err("missing ack must fail");
        assert!(error.contains("timed out"));
        client.shutdown();
        let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn server_replacement_stops_worker_and_reports_terminal_reason() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("websocket");
            let subscribe = next_json(&mut socket).await;
            acknowledge(&mut socket, &subscribe).await;
            tokio::time::sleep(Duration::from_millis(30)).await;
            socket
                .send(text_message(&json!({
                    "cmd":EVENT_CALLBACK,
                    "headers":{"req_id":"disconnect-event"},
                    "body":{"msgtype":"event","event":{"eventtype":"disconnected_event"}}
                })))
                .await
                .expect("disconnect event");
        });
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();
        let options = ClientOptions {
            url: format!("ws://{address}"),
            heartbeat_interval: Duration::from_secs(1),
            reconnect_base_delay: Duration::from_millis(20),
            connect_timeout: Duration::from_secs(1),
            reply_timeout: Duration::from_secs(1),
        };
        let (client, task) = WecomClient::connect_configured(
            "bot-id".into(),
            "bot-secret".into(),
            |_| async {},
            move |error| {
                let _ = status_tx.send(error);
            },
            options,
        )
        .await
        .expect("client");
        let error = tokio::time::timeout(Duration::from_secs(1), status_rx.recv())
            .await
            .expect("status timeout")
            .expect("status");
        assert!(error.contains("replaced"));
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("task timeout")
            .expect("task");
        assert!(!client.authenticated.load(Ordering::Acquire));
        server.await.expect("server");
    }

    #[tokio::test]
    async fn reconnects_and_authenticates_before_dispatching_again() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            for connection in 0..2 {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut socket = tokio_tungstenite::accept_async(stream)
                    .await
                    .expect("websocket");
                let subscribe = next_json(&mut socket).await;
                assert_eq!(subscribe["cmd"], SUBSCRIBE);
                acknowledge(&mut socket, &subscribe).await;
                if connection == 0 {
                    socket.close(None).await.expect("close first connection");
                } else {
                    socket
                        .send(text_message(&json!({
                            "cmd":MESSAGE_CALLBACK,
                            "headers":{"req_id":"callback_after_reconnect"},
                            "body":{"chatid":"chat-2","from":{"userid":"user-2"},"text":{"content":"again"}}
                        })))
                        .await
                        .expect("callback after reconnect");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        });

        let (callback_tx, mut callback_rx) = mpsc::unbounded_channel();
        let options = ClientOptions {
            url: format!("ws://{address}"),
            heartbeat_interval: Duration::from_secs(1),
            reconnect_base_delay: Duration::from_millis(20),
            connect_timeout: Duration::from_secs(2),
            reply_timeout: Duration::from_secs(1),
        };
        let (client, task) = WecomClient::connect_with_options(
            "bot-id".into(),
            "bot-secret".into(),
            move |frame| {
                let callback_tx = callback_tx.clone();
                async move {
                    let _ = callback_tx.send(frame);
                }
            },
            options,
        )
        .await
        .expect("initial connection");

        let callback = tokio::time::timeout(Duration::from_secs(2), callback_rx.recv())
            .await
            .expect("reconnect callback timeout")
            .expect("reconnect callback");
        assert_eq!(callback["body"]["text"]["content"], "again");
        client.shutdown();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("client stop timeout")
            .expect("client task");
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn async_callback_applies_backpressure_without_dropping_bursts() {
        const MESSAGE_COUNT: usize = 256;
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let (server_done_tx, server_done_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("websocket");
            let subscribe = next_json(&mut socket).await;
            acknowledge(&mut socket, &subscribe).await;
            for index in 0..MESSAGE_COUNT {
                socket
                    .send(text_message(&json!({
                        "cmd":MESSAGE_CALLBACK,
                        "headers":{"req_id":format!("callback-{index}")},
                        "body":{"chatid":"chat","msgid":format!("msg-{index}"),
                            "msgtype":"text","from":{"userid":"user"},
                            "text":{"content":index.to_string()}}
                    })))
                    .await
                    .expect("callback");
            }
            let _ = server_done_rx.await;
        });
        let (callback_tx, mut callback_rx) = mpsc::channel(1);
        let options = ClientOptions {
            url: format!("ws://{address}"),
            heartbeat_interval: Duration::from_secs(30),
            reconnect_base_delay: Duration::from_millis(20),
            connect_timeout: Duration::from_secs(2),
            reply_timeout: Duration::from_secs(1),
        };
        let (client, task) = WecomClient::connect_with_options(
            "bot-id".into(),
            "bot-secret".into(),
            move |frame| {
                let callback_tx = callback_tx.clone();
                async move {
                    callback_tx.send(frame).await.expect("callback receiver");
                }
            },
            options,
        )
        .await
        .expect("client");

        for index in 0..MESSAGE_COUNT {
            let frame = tokio::time::timeout(Duration::from_secs(2), callback_rx.recv())
                .await
                .expect("callback timeout")
                .expect("callback");
            assert_eq!(frame["body"]["text"]["content"], index.to_string());
        }
        let _ = server_done_tx.send(());
        client.shutdown();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("client stop timeout")
            .expect("client task");
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn supports_callback_stream_replies_and_media_upload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("websocket");
            let subscribe = next_json(&mut socket).await;
            acknowledge(&mut socket, &subscribe).await;
            socket
                .send(text_message(&json!({
                    "cmd":MESSAGE_CALLBACK,
                    "headers":{"req_id":"callback_stream_1"},
                    "body":{"chatid":"chat-stream","msgid":"msg-stream","msgtype":"text","from":{"userid":"user"},"text":{"content":"go"}}
                })))
                .await
                .expect("callback");

            let stream_reply = next_json(&mut socket).await;
            assert_eq!(stream_reply["cmd"], RESPOND_MSG);
            assert_eq!(stream_reply["headers"]["req_id"], "callback_stream_1");
            assert_eq!(stream_reply["body"]["msgtype"], "stream");
            assert_eq!(stream_reply["body"]["stream"]["finish"], false);
            acknowledge(&mut socket, &stream_reply).await;

            let welcome = next_json(&mut socket).await;
            assert_eq!(welcome["cmd"], RESPOND_WELCOME);
            assert_eq!(welcome["headers"]["req_id"], "welcome-1");
            acknowledge(&mut socket, &welcome).await;

            let update = next_json(&mut socket).await;
            assert_eq!(update["cmd"], RESPOND_UPDATE);
            assert_eq!(update["body"]["response_type"], "update_template_card");
            assert_eq!(update["body"]["userids"][0], "user");
            acknowledge(&mut socket, &update).await;

            let init = next_json(&mut socket).await;
            assert_eq!(init["cmd"], UPLOAD_MEDIA_INIT);
            assert_eq!(init["body"]["total_chunks"], 1);
            socket
                .send(text_message(&json!({
                    "headers":{"req_id":init["headers"]["req_id"]},
                    "errcode":0,"body":{"upload_id":"upload-1"}
                })))
                .await
                .expect("init ack");

            let chunk = next_json(&mut socket).await;
            assert_eq!(chunk["cmd"], UPLOAD_MEDIA_CHUNK);
            assert_eq!(chunk["body"]["chunk_index"], 0);
            assert_eq!(chunk["body"]["base64_data"], "aW1hZ2UtYnl0ZXM=");
            acknowledge(&mut socket, &chunk).await;

            let finish = next_json(&mut socket).await;
            assert_eq!(finish["cmd"], UPLOAD_MEDIA_FINISH);
            socket
                .send(text_message(&json!({
                    "headers":{"req_id":finish["headers"]["req_id"]},
                    "errcode":0,"body":{"media_id":"media-1","type":"image"}
                })))
                .await
                .expect("finish ack");
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        let (callback_tx, mut callback_rx) = mpsc::unbounded_channel();
        let options = ClientOptions {
            url: format!("ws://{address}"),
            heartbeat_interval: Duration::from_secs(1),
            reconnect_base_delay: Duration::from_millis(20),
            connect_timeout: Duration::from_secs(2),
            reply_timeout: Duration::from_secs(1),
        };
        let (client, task) = WecomClient::connect_with_options(
            "bot-id".into(),
            "bot-secret".into(),
            move |frame| {
                let callback_tx = callback_tx.clone();
                async move {
                    let _ = callback_tx.send(frame);
                }
            },
            options,
        )
        .await
        .expect("client");
        callback_rx.recv().await.expect("callback");
        assert_eq!(
            client.reply_req_id("chat-stream").as_deref(),
            Some("callback_stream_1")
        );
        client
            .reply_message(
                "callback_stream_1",
                json!({"msgtype":"stream","stream":{"id":"stream-1","finish":false,"content":"partial"}}),
            )
            .await
            .expect("stream reply");
        client
            .reply_welcome(
                "welcome-1",
                json!({"msgtype":"text","text":{"content":"welcome"}}),
            )
            .await
            .expect("welcome reply");
        client
            .update_template_card(
                "card-1",
                json!({"card_type":"text_notice"}),
                &["user".into()],
            )
            .await
            .expect("card update");
        assert_eq!(
            client
                .upload_media(b"image-bytes", "image", "image.png")
                .await
                .expect("upload"),
            "media-1"
        );
        client.shutdown();
        let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn missing_heartbeat_ack_forces_authenticated_reconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (first, _) = listener.accept().await.expect("first accept");
            let mut first = tokio_tungstenite::accept_async(first)
                .await
                .expect("first ws");
            let subscribe = next_json(&mut first).await;
            acknowledge(&mut first, &subscribe).await;
            for _ in 0..2 {
                let heartbeat = next_json(&mut first).await;
                assert_eq!(heartbeat["cmd"], HEARTBEAT);
            }
            let (second, _) = listener.accept().await.expect("second accept");
            let mut second = tokio_tungstenite::accept_async(second)
                .await
                .expect("second ws");
            let subscribe = next_json(&mut second).await;
            acknowledge(&mut second, &subscribe).await;
            second
                .send(text_message(&json!({
                    "cmd":MESSAGE_CALLBACK,
                    "headers":{"req_id":"after-heartbeat-reconnect"},
                    "body":{"chatid":"chat","msgid":"msg","msgtype":"text","from":{"userid":"user"},"text":{"content":"alive"}}
                })))
                .await
                .expect("callback");
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let (callback_tx, mut callback_rx) = mpsc::unbounded_channel();
        let options = ClientOptions {
            url: format!("ws://{address}"),
            heartbeat_interval: Duration::from_millis(30),
            reconnect_base_delay: Duration::from_millis(20),
            connect_timeout: Duration::from_secs(2),
            reply_timeout: Duration::from_secs(1),
        };
        let (client, task) = WecomClient::connect_with_options(
            "bot-id".into(),
            "bot-secret".into(),
            move |frame| {
                let callback_tx = callback_tx.clone();
                async move {
                    let _ = callback_tx.send(frame);
                }
            },
            options,
        )
        .await
        .expect("client");
        let callback = tokio::time::timeout(Duration::from_secs(2), callback_rx.recv())
            .await
            .expect("callback timeout")
            .expect("callback");
        assert_eq!(callback["body"]["text"]["content"], "alive");
        client.shutdown();
        let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
        server.await.expect("server task");
    }

    async fn next_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        loop {
            match socket
                .next()
                .await
                .expect("websocket message")
                .expect("frame")
            {
                Message::Text(text) => return serde_json::from_str(&text).expect("json"),
                Message::Binary(bytes) => return serde_json::from_slice(&bytes).expect("json"),
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.expect("pong"),
                Message::Pong(_) | Message::Frame(_) => {}
                Message::Close(frame) => panic!("unexpected close: {frame:?}"),
            }
        }
    }

    async fn acknowledge<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, frame: &Value)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        socket
            .send(text_message(&json!({
                "headers":{"req_id":frame["headers"]["req_id"]},
                "errcode":0,
                "errmsg":"ok"
            })))
            .await
            .expect("acknowledgement");
    }
}
