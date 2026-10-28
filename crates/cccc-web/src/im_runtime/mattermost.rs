use super::bridge_log::append as append_log;
use super::mattermost_inbound::MattermostInbound;
use super::mattermost_outbound::MattermostOutbound;
use super::processing_reactions::{Active, reaction_request, spawn_processing_cleanup};
use super::{
    completes_processing, is_outbound_or_stream, processing_reply_to, resolve_config_credential,
    spawn_outbound_matching,
};
use cccc_client::DaemonClient;
use cccc_core::{GroupStore, HomeLayout};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Method, StatusCode};
use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::{client::generate_key, derive_accept_key},
        protocol::Role,
    },
};

pub(super) const PLATFORM: &str = "mattermost";
type Socket = WebSocketStream<reqwest::Upgraded>;

async fn socket_send(
    socket: &mut (impl futures_util::Sink<Message> + Unpin),
    message: Message,
) -> Result<(), &'static str> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message))
        .await
        .map_err(|_| "Mattermost WebSocket write timed out")?
        .map_err(|_| "Mattermost WebSocket write failed")
}

#[derive(Clone)]
struct WorkerState {
    config: Map<String, Value>,
    generations: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
    generation: u64,
}

impl WorkerState {
    fn is_current(&self, group_id: &str, state: &Value) -> bool {
        state.get("config").and_then(Value::as_object) == Some(&self.config)
            && self.generations.lock().is_ok_and(|generations| {
                generations.get(group_id).copied() == Some(self.generation)
            })
    }
}

#[derive(Debug, Default)]
struct SocketCursor {
    connection_id: String,
    next_sequence: u64,
    missed_events: bool,
}

const RECOVERY_GAP: &str = "Mattermost WebSocket recovery cache expired; some messages may be missing. Please resend unanswered requests.";

impl SocketCursor {
    // Mattermost seq is connection-wide, not a post_id; non-posted events must advance it too.
    fn accept(&mut self, event: &Value) -> Result<bool, &'static str> {
        if event.get("seq_reply").is_some() {
            return Ok(false);
        }
        if field(event, "event") == "hello" {
            let id = field(&event["data"], "connection_id");
            if !valid_id(id) {
                return Err("Mattermost hello has no valid connection id");
            }
            if !self.connection_id.is_empty() && self.connection_id != id {
                self.missed_events = true;
                self.next_sequence = 0;
            }
            self.connection_id = id.to_owned();
        }
        let sequence = event["seq"]
            .as_u64()
            .ok_or("Mattermost event has no valid sequence")?;
        if sequence < self.next_sequence {
            return Ok(false);
        }
        if sequence != self.next_sequence {
            return Err("Mattermost WebSocket sequence gap; reconnecting to recover missed events");
        }
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or("Mattermost event sequence overflow")?;
        Ok(true)
    }
}

#[derive(Debug)]
enum SocketError {
    Retry(String),
    Authentication(String),
}

impl std::fmt::Display for SocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retry(message) | Self::Authentication(message) => f.write_str(message),
        }
    }
}

impl From<String> for SocketError {
    fn from(message: String) -> Self {
        Self::Retry(message)
    }
}

impl From<&str> for SocketError {
    fn from(message: &str) -> Self {
        Self::Retry(message.to_owned())
    }
}

#[derive(Clone)]
pub(super) struct MattermostApi {
    pub http: reqwest::Client,
    site: String,
    token: String,
    pub bot_id: String,
    pub username: String,
    worker: Option<WorkerState>,
}

impl MattermostApi {
    pub(super) fn log_error(
        &self,
        home: &HomeLayout,
        group_id: &str,
        operation: &str,
        error: &str,
    ) {
        log_error(home, group_id, operation, error, &self.token);
    }

    pub(super) async fn authenticate(
        config: &Map<String, Value>,
        token: String,
    ) -> Result<Self, String> {
        let site = config
            .get("mattermost_url")
            .and_then(Value::as_str)
            .and_then(cccc_core::im_state::normalize_mattermost_url)
            .ok_or("Mattermost site URL is invalid")?;
        let http = reqwest::Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(http_error)?;
        let mut api = Self {
            http,
            site,
            token,
            bot_id: String::new(),
            username: String::new(),
            worker: None,
        };
        let user = api.json(Method::GET, "users/me", None).await?;
        api.bot_id = field(&user, "id").to_owned();
        api.username = field(&user, "username").to_owned();
        if !valid_id(&api.bot_id)
            || api.username.is_empty()
            || user["is_bot"].as_bool() != Some(true)
        {
            return Err("Mattermost credentials must belong to a Bot account".into());
        }
        Ok(api)
    }

    pub(super) fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/api/v4/{path}", self.site))
            .bearer_auth(&self.token)
    }

    pub(super) async fn response(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        // Retry only explicit rate-limit rejections; a lost connection does not prove that post creation failed.
        for attempt in 0..3 {
            let candidate = request
                .try_clone()
                .ok_or("Mattermost request body cannot be replayed")?;
            let response = candidate.send().await.map_err(http_error)?;
            if response.status() == StatusCode::TOO_MANY_REQUESTS && attempt < 2 {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(2)
                    .clamp(1, 60);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                continue;
            }
            if !response.status().is_success() {
                return Err(format!(
                    "Mattermost API returned HTTP {} for {}",
                    response.status().as_u16(),
                    response.url().path()
                ));
            }
            return Ok(response);
        }
        Err("Mattermost rate limit exceeded".into())
    }

    pub(super) async fn json(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let mut request = self.request(method, path);
        if let Some(body) = body {
            request = request.json(&body);
        }
        self.response(request)
            .await?
            .json()
            .await
            .map_err(http_error)
    }

    pub(super) async fn post(
        &self,
        chat_id: &str,
        thread_id: &str,
        text: &str,
        files: &[String],
    ) -> Result<String, String> {
        if !valid_id(chat_id) || (!thread_id.is_empty() && !valid_id(thread_id)) {
            return Err("Invalid Mattermost chat or thread id".into());
        }
        let post = self
            .json(
                Method::POST,
                "posts",
                Some(json!({
                    "channel_id":chat_id, "root_id":thread_id, "message":text, "file_ids":files
                })),
            )
            .await?;
        let id = field(&post, "id");
        if !valid_id(id) {
            return Err("Mattermost post response has no valid id".into());
        }
        Ok(id.to_owned())
    }

    pub(super) async fn edit(&self, post_id: &str, text: &str) -> Result<(), String> {
        if !valid_id(post_id) {
            return Err("Invalid Mattermost post id".into());
        }
        self.json(
            Method::PUT,
            &format!("posts/{post_id}/patch"),
            Some(json!({"message":text})),
        )
        .await?;
        Ok(())
    }

    async fn open_socket(&self, cursor: &SocketCursor) -> Result<Socket, SocketError> {
        // Reuse reqwest TLS and HTTP_PROXY/HTTPS_PROXY/NO_PROXY for the HTTP upgrade.
        let key = generate_key();
        let response = self
            .request(Method::GET, "websocket")
            .query(&[
                ("connection_id", cursor.connection_id.clone()),
                ("sequence_number", cursor.next_sequence.to_string()),
            ])
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", &key)
            .send()
            .await
            .map_err(http_error)?;
        if response.status() != StatusCode::SWITCHING_PROTOCOLS
            || response
                .headers()
                .get("sec-websocket-accept")
                .and_then(|v| v.to_str().ok())
                != Some(derive_accept_key(key.as_bytes()).as_str())
        {
            let message = format!(
                "Mattermost WebSocket upgrade failed (HTTP {})",
                response.status().as_u16()
            );
            return Err(
                if matches!(
                    response.status(),
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
                ) {
                    SocketError::Authentication(message)
                } else {
                    SocketError::Retry(message)
                },
            );
        }
        let upgraded = response.upgrade().await.map_err(http_error)?;
        Ok(WebSocketStream::from_raw_socket(upgraded, Role::Client, None).await)
    }

    async fn socket(&self) -> Result<(Socket, SocketCursor), SocketError> {
        let mut cursor = SocketCursor::default();
        let mut socket = self.open_socket(&cursor).await?;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                match socket.next().await {
                    Some(Ok(Message::Text(text))) => {
                        let event: Value = serde_json::from_str(&text)
                            .map_err(|_| "Invalid Mattermost WebSocket JSON")?;
                        if field(&event, "event") == "hello" {
                            cursor.accept(&event)?;
                            return Ok::<(), SocketError>(());
                        }
                        if event.get("error").is_some_and(|v| !v.is_null()) {
                            return Err(SocketError::Authentication(
                                "Mattermost WebSocket authentication failed".into(),
                            ));
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        socket
                            .send(Message::Pong(data))
                            .await
                            .map_err(|_| "Mattermost WebSocket ping failed")?;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        return Err("Mattermost WebSocket closed before hello".into());
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| "Mattermost WebSocket hello timed out")??;
        Ok((socket, cursor))
    }
}

pub(super) fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 26
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn http_error(error: reqwest::Error) -> String {
    error.without_url().to_string()
}

pub(super) async fn start_registered(
    registry: &super::ImWorkerRegistry,
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    generation: u64,
) -> Result<(), String> {
    let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
    // Clear the previous error before spawning workers, not after a worker may have failed.
    update_start_state(registry, &store, group_id, config, generation, None)?;
    let result = match start(
        home,
        daemon,
        group_id,
        config,
        registry.ledger_events.clone(),
        WorkerState {
            config: config.clone(),
            generations: registry.generations.clone(),
            generation,
        },
    )
    .await
    {
        Ok(tasks) => {
            registry
                .install(
                    group_id,
                    generation,
                    super::worker(tasks, super::no_op_stopper()),
                )
                .await
        }
        Err(error) => Err(error),
    };
    complete_start(registry, &store, group_id, config, generation, result).await
}

async fn complete_start(
    registry: &super::ImWorkerRegistry,
    store: &GroupStore,
    group_id: &str,
    config: &Map<String, Value>,
    generation: u64,
    result: Result<(), String>,
) -> Result<(), String> {
    if let Err(error) =
        update_start_state(registry, store, group_id, config, generation, Some(&result))
    {
        // Use the install/stop lifecycle lock and remove only this failed generation.
        let lifecycle_lock = registry.lifecycle_lock(group_id);
        let worker = {
            let _guard = lifecycle_lock.lock().await;
            if registry.is_generation_current(group_id, generation) {
                registry
                    .generations
                    .lock()
                    .expect("IM generation registry poisoned")
                    .remove(group_id);
                registry
                    .workers
                    .lock()
                    .expect("IM worker registry poisoned")
                    .remove(group_id)
            } else {
                None
            }
        };
        if let Some(worker) = worker {
            worker.shutdown().await;
        }
        return Err(error);
    }
    result
}

fn update_start_state(
    registry: &super::ImWorkerRegistry,
    store: &GroupStore,
    group_id: &str,
    config: &Map<String, Value>,
    generation: u64,
    result: Option<&Result<(), String>>,
) -> Result<(), String> {
    cccc_core::im_state::update(store, group_id, |state| {
        // Check inside the same file lock used by save; identical config saves also invalidate
        // the native generation. A snapshot-only check would miss stop/save/start races.
        if state.get("config").and_then(Value::as_object) != Some(config)
            || !registry.is_generation_current(group_id, generation)
        {
            return Err(std::io::Error::other(
                "IM worker start was superseded by a newer request",
            ));
        }
        let running = result.is_some_and(Result::is_ok) && registry.is_running(group_id);
        state["enabled"] = json!(true);
        state["running"] = json!(running);
        state["pid"] = if running {
            json!(std::process::id())
        } else {
            Value::Null
        };
        state["adapter_available"] = json!(running);
        if let Some(Err(error)) = result {
            state["last_error"] = json!(error);
        } else if result.is_none() {
            state["last_error"] = Value::Null;
        }
        state["updated_at"] = json!(cccc_contracts::utc_now());
        Ok(())
    })
    .map(|_| ())
    .map_err(|error| error.to_string())
}

async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
    worker: WorkerState,
) -> Result<Vec<JoinHandle<()>>, String> {
    let token = resolve_config_credential(config, "bot_token", "bot_token_env")
        .inspect_err(|error| log_error(&home, group_id, "credential", error, ""))?;
    let mut api = MattermostApi::authenticate(config, token.clone())
        .await
        .inspect_err(|error| {
            log_error(&home, group_id, "authenticate", error, &token);
        })?;
    api.worker = Some(worker);
    let socket = api
        .socket()
        .await
        .map_err(|error| error.to_string())
        .inspect_err(|error| {
            log_error(&home, group_id, "connect", error, &token);
        })?;
    verify_identity(&home, group_id, &api, config).inspect_err(|error| {
        log_error(&home, group_id, "identity", error, &token);
    })?;
    let reactions = MattermostReactions::new(home.clone(), group_id, api.clone());
    let mut inbound = MattermostInbound::new(
        home.clone(),
        group_id,
        daemon,
        api.clone(),
        reactions.clone(),
        config,
    );
    // Follow WeCom's bounded queue and separate inbound task so attachment requests do not block heartbeats.
    let (inbound_tx, mut inbound_rx) = mpsc::channel(128);
    let inbound_home = home.clone();
    let inbound_group = group_id.to_owned();
    let inbound_api = api.clone();
    let inbound = tokio::spawn(async move {
        while let Some(event) = inbound_rx.recv().await {
            if let Err(error) = inbound.handle(&event).await {
                inbound_api.log_error(&inbound_home, &inbound_group, "inbound", &error);
                inbound_api.persist_error(&inbound_home, &inbound_group, Some(&error));
            }
        }
    });
    let connection = tokio::spawn(socket_loop(
        home.clone(),
        group_id.to_owned(),
        api.clone(),
        socket,
        inbound_tx,
        Duration::from_secs(30),
    ));
    let cleanup = reactions.cleanup_task();
    let error_api = api.clone();
    let outbound = spawn_outbound_matching(
        home.clone(),
        group_id.to_owned(),
        PLATFORM,
        ledger_events,
        MattermostOutbound::new(home.clone(), group_id, api, config),
        is_outbound_or_stream,
        move |sender, targets, event| {
            let reactions = reactions.clone();
            let home = home.clone();
            let token = token.clone();
            let error_api = error_api.clone();
            async move {
                for target in targets {
                    let result = sender.send_target(&target, &event).await;
                    if let Err(error) = &result {
                        log_error(&home, &event.group_id, "send", error, &token);
                        error_api.persist_error(&home, &event.group_id, Some(error));
                    }
                    if completes_processing(&event) {
                        reactions
                            .complete(&target.key(), processing_reply_to(&event), result.is_ok())
                            .await;
                    }
                }
            }
        },
    );
    Ok(vec![connection, inbound, outbound, cleanup])
}

fn verify_identity(
    home: &HomeLayout,
    group_id: &str,
    api: &MattermostApi,
    config: &Map<String, Value>,
) -> Result<(), String> {
    verify_identity_with(home, group_id, api, config, |path, identity| {
        cccc_core::fs::write_json_committed(path, identity)
    })
}

fn verify_identity_with(
    home: &HomeLayout,
    group_id: &str,
    api: &MattermostApi,
    config: &Map<String, Value>,
    commit: impl FnOnce(&std::path::Path, &Value) -> std::io::Result<()>,
) -> Result<(), String> {
    let store = GroupStore::new(home.clone()).map_err(|e| e.to_string())?;
    let path = store
        .state_dir(group_id)
        .map_err(|e| e.to_string())?
        .join("mattermost_identity.json");
    let expected = json!({"site":api.site,"bot_id":api.bot_id});
    // Serialize the whole sequence. Do not move the identity write into im_state::update:
    // authorization commits after the callback; clearing it must precede recording the new identity.
    cccc_core::fs::with_exclusive_lock(&path.with_extension("lock"), || {
        let previous: Value = match cccc_core::fs::read_json(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
            Err(error) => return Err(error),
        };
        cccc_core::im_state::update(&store, group_id, |state| {
            // A stale startup must not change authorization for a config replaced by another save.
            if state.get("config").and_then(Value::as_object) != Some(config)
                || api
                    .worker
                    .as_ref()
                    .is_some_and(|worker| !worker.is_current(group_id, state))
            {
                return Err(std::io::Error::other(
                    "Mattermost configuration changed during startup",
                ));
            }
            if previous != expected {
                for key in ["authorized", "pending", "subscribers"] {
                    state[key] = json!([]);
                }
            }
            Ok(())
        })?;
        // Persist authorization removal before identity; retrying an interrupted change must not reuse old grants.
        commit(&path, &expected)
    })
    .map_err(|e| e.to_string())
}

// The combined CLI/Web entry point has no tracing subscriber; Group logs must work independently.
pub(super) fn log_error(
    home: &HomeLayout,
    group_id: &str,
    operation: &str,
    error: &str,
    token: &str,
) {
    let error = if token.is_empty() {
        error.to_owned()
    } else {
        error.replace(token, "[REDACTED]")
    };
    let line = json!({
        "ts": cccc_contracts::utc_now(), "level": "WARN", "platform": PLATFORM,
        "group_id": group_id, "operation": operation,
        "error": error.chars().take(4096).collect::<String>()
    })
    .to_string();
    cccc_runtime::diagnostic(format_args!("{line}"));
    if let Err(error) = append_log(home, group_id, &line) {
        cccc_runtime::diagnostic(format_args!(
            "Mattermost group log write failed ({:?})",
            error.kind()
        ));
    }
}

impl MattermostApi {
    fn persist_error(&self, home: &HomeLayout, group_id: &str, error: Option<&str>) {
        // Standalone API clients do not own runtime state; only a registered worker can update it.
        let Some(worker) = &self.worker else {
            return;
        };
        let result = GroupStore::new(home.clone()).and_then(|store| {
            cccc_core::im_state::update(&store, group_id, |state| {
                if !worker.is_current(group_id, state) {
                    return Err(std::io::Error::other("Mattermost worker was superseded"));
                }
                state["last_error"] =
                    error.map_or(Value::Null, |v| json!(v.replace(&self.token, "[REDACTED]")));
                Ok(())
            })
        });
        if let Err(error) = result {
            cccc_runtime::diagnostic(format_args!(
                "Mattermost error state write failed ({:?})",
                error.kind()
            ));
        }
    }
}

async fn socket_loop(
    home: HomeLayout,
    group_id: String,
    api: MattermostApi,
    connection: (Socket, SocketCursor),
    inbound: mpsc::Sender<Value>,
    heartbeat_interval: Duration,
) {
    let (mut socket, mut cursor) = connection;
    loop {
        let mut heartbeat = tokio::time::interval(heartbeat_interval);
        let mut last_received = tokio::time::Instant::now();
        let mut permit = None;
        let error = loop {
            tokio::select! {
                capacity = inbound.reserve(), if permit.is_none() => {
                    let Ok(capacity) = capacity else { return; };
                    permit = Some(capacity);
                    // Local backpressure is not remote inactivity; restart the receive timeout when reading resumes.
                    last_received = tokio::time::Instant::now();
                }
                message = socket.next(), if permit.is_some() => {
                    last_received = tokio::time::Instant::now();
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<Value>(&text) {
                                Ok(event) => {
                                    if event.get("error").is_some_and(|v| !v.is_null()) {
                                        let error = "Mattermost WebSocket authentication failed";
                                        api.persist_error(&home, &group_id, Some(error));
                                        api.log_error(&home, &group_id, "authenticate", error);
                                        return;
                                    }
                                    let had_gap = cursor.missed_events;
                                    match cursor.accept(&event) {
                                        Ok(true) => {
                                            if !had_gap && cursor.missed_events {
                                                api.log_error(&home, &group_id, "recovery", RECOVERY_GAP);
                                                api.persist_error(&home, &group_id, Some(RECOVERY_GAP));
                                            }
                                            if field(&event, "event") == "posted" {
                                                if let Some(capacity) = permit.take() { capacity.send(event); }
                                            }
                                        }
                                        Ok(false) => {}
                                        Err(error) => break error,
                                    }
                                }
                                Err(_) => break "invalid Mattermost event JSON",
                            }
                        }
                        Some(Ok(Message::Ping(data))) => { if let Err(error) = socket_send(&mut socket, Message::Pong(data)).await { break error; } }
                        Some(Ok(Message::Close(_))) | None => break "Mattermost WebSocket disconnected",
                        Some(Err(_)) => break "Mattermost WebSocket read failed",
                        _ => {}
                    }
                }
                _ = heartbeat.tick() => {
                    if permit.is_some() && last_received.elapsed() > heartbeat_interval * 3 { break "Mattermost WebSocket heartbeat timed out"; }
                    if let Err(error) = socket_send(&mut socket, Message::Ping(Vec::new().into())).await { break error; }
                }
            }
        };
        api.persist_error(&home, &group_id, Some(error));
        log_error(&home, &group_id, "disconnect", error, &api.token);
        drop(permit);
        // Close the old socket so the server can make it resumable. Do not wait for hello on reconnect:
        // resuming the same ID sends cached events directly, or no application events if nothing is pending.
        drop(socket);
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            match api.open_socket(&cursor).await {
                Ok(next) => {
                    socket = next;
                    api.persist_error(
                        &home,
                        &group_id,
                        cursor.missed_events.then_some(RECOVERY_GAP),
                    );
                    break;
                }
                Err(error) => {
                    let message = error.to_string();
                    api.persist_error(&home, &group_id, Some(&message));
                    log_error(&home, &group_id, "reconnect", &message, &api.token);
                    if matches!(error, SocketError::Authentication(_)) {
                        return;
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub(super) struct MattermostReactions {
    home: HomeLayout,
    group_id: String,
    api: MattermostApi,
    active: Active<MattermostReaction>,
    pub(super) binding: Arc<Mutex<()>>,
}
#[derive(Clone)]
struct MattermostReaction {
    post_id: String,
    event_id: String,
}

impl MattermostReactions {
    pub(super) fn new(home: HomeLayout, group_id: &str, api: MattermostApi) -> Self {
        Self {
            home,
            group_id: group_id.to_owned(),
            api,
            active: Active::default(),
            binding: Arc::default(),
        }
    }

    pub(super) async fn start(&self, key: &str, post_id: &str) {
        self.active.push(
            key.to_owned(),
            MattermostReaction {
                post_id: post_id.to_owned(),
                event_id: String::new(),
            },
        );
        if let Err(error) = reaction_request(self.emoji(post_id, "eyes")).await {
            log_error(
                &self.home,
                &self.group_id,
                "reaction_start",
                &error,
                &self.api.token,
            );
        }
    }
    pub(super) fn bind(&self, key: &str, post_id: &str, event_id: String) {
        self.active
            .update_where(key, |r| r.post_id == post_id, |r| r.event_id = event_id);
    }
    pub(super) async fn fail_post(&self, key: &str, post_id: &str) {
        self.finish(
            self.active.take_where(key, |r| r.post_id == post_id),
            Some(false),
        )
        .await;
    }
    pub(super) async fn unknown_post(&self, key: &str, post_id: &str) {
        // When acceptance is unknown, remove the processing marker without marking a possibly successful submission as failed.
        self.finish(self.active.take_where(key, |r| r.post_id == post_id), None)
            .await;
    }
    async fn complete(&self, key: &str, reply_to: Option<&str>, success: bool) {
        let binding = self.binding.lock().await;
        let reaction = match reply_to {
            Some(id) => self.active.take_where(key, |r| r.event_id == id),
            None if self.active.len(key) == 1 => self.active.take_next(key),
            None => None,
        };
        drop(binding);
        self.finish(reaction, Some(success)).await;
    }
    async fn emoji(&self, post_id: &str, emoji: &str) -> Result<(), String> {
        self.api
            .json(
                Method::POST,
                "reactions",
                Some(json!({"user_id":self.api.bot_id,"post_id":post_id,"emoji_name":emoji})),
            )
            .await?;
        Ok(())
    }
    async fn finish(&self, reaction: Option<MattermostReaction>, success: Option<bool>) {
        let Some(reaction) = reaction else {
            return;
        };
        let path = format!(
            "users/{}/posts/{}/reactions/eyes",
            self.api.bot_id, reaction.post_id
        );
        if let Err(error) =
            reaction_request(self.api.response(self.api.request(Method::DELETE, &path))).await
        {
            log_error(
                &self.home,
                &self.group_id,
                "reaction_cleanup",
                &error,
                &self.api.token,
            );
        }
        let Some(success) = success else {
            return;
        };
        if let Err(error) = reaction_request(self.emoji(
            &reaction.post_id,
            if success { "white_check_mark" } else { "x" },
        ))
        .await
        {
            log_error(
                &self.home,
                &self.group_id,
                "reaction_finish",
                &error,
                &self.api.token,
            );
        }
    }
    fn cleanup_task(&self) -> JoinHandle<()> {
        let reactions = self.clone();
        spawn_processing_cleanup(move || {
            let reactions = reactions.clone();
            async move {
                for reaction in reactions.active.take_expired() {
                    reactions.finish(Some(reaction), Some(false)).await;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
