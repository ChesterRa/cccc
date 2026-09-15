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
use std::io::Write;
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
    eprintln!("{line}");
    if let Err(error) = append_log(home, group_id, &line) {
        eprintln!("Mattermost group log write failed ({:?})", error.kind());
    }
}

fn append_log(home: &HomeLayout, group_id: &str, line: &str) -> std::io::Result<()> {
    let dir = GroupStore::new(home.clone())?.state_dir(group_id)?;
    cccc_core::fs::with_exclusive_lock(&dir.join("im_bridge.log.lock"), || {
        let path = dir.join("im_bridge.log");
        if path.exists() && path.metadata()?.len() + line.len() as u64 + 1 > 1024 * 1024 {
            let backup = dir.join("im_bridge.log.1");
            if backup.exists() {
                std::fs::remove_file(&backup)?;
            }
            std::fs::rename(&path, backup)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        writeln!(file, "{line}")?;
        file.sync_data()
    })
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
            eprintln!("Mattermost error state write failed ({:?})", error.kind());
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
mod tests {
    // Non-ASCII fixture bodies and filenames verify Unicode chunking and file round trips.
    use super::super::{AuthorizedChat, authorized_chats};
    use super::*;
    use axum::{
        Router,
        extract::{Request, State, ws::WebSocketUpgrade},
        http::HeaderMap,
        response::{IntoResponse, Response},
        routing::{any, get},
    };
    use cccc_contracts::Event;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::Instant;

    #[derive(Default)]
    struct MockState {
        posts: Mutex<Vec<Value>>,
        fail_edit: AtomicBool,
        fail_create: AtomicBool,
        ws_mode: AtomicUsize,
        ws_connections: AtomicUsize,
        ws_attempts: AtomicUsize,
        ws_queries: Mutex<Vec<std::collections::HashMap<String, String>>>,
        blocked_identity: AtomicBool,
        identity_requests: AtomicUsize,
        identity_entered: tokio::sync::Notify,
        release_identity: tokio::sync::Notify,
        ws_events: Mutex<Vec<Value>>,
        ws_pings: AtomicUsize,
        ws_pongs: AtomicUsize,
        blocked_download: AtomicBool,
        release_download: tokio::sync::Notify,
        uploads: Mutex<Vec<Vec<u8>>>,
        upload_metadata: Mutex<Vec<Value>>,
        downloads: Mutex<usize>,
        inbound_files: Mutex<std::collections::HashMap<String, Value>>,
        file_stream_entered: tokio::sync::Notify,
        rate_requests: Mutex<usize>,
        reactions: Mutex<Vec<(Method, String, Value)>>,
        forbidden: AtomicBool,
        other_user_is_bot: AtomicBool,
        fail_lookup: AtomicUsize,
        lookup_calls: AtomicUsize,
        failed_posts: AtomicUsize,
        token: String,
        bot_id: String,
    }

    struct Fixture {
        api: MattermostApi,
        state: Arc<MockState>,
        task: JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn fixture() -> Fixture {
        fixture_for_bot("test-token", &"b".repeat(26)).await
    }

    async fn authenticate(config: &Map<String, Value>) -> Result<MattermostApi, String> {
        let token = resolve_config_credential(config, "bot_token", "bot_token_env")?;
        MattermostApi::authenticate(config, token).await
    }

    fn worker_state(home: &HomeLayout, group: &str) -> WorkerState {
        let store = GroupStore::new(home.clone()).expect("store");
        cccc_core::im_state::update(&store, group, |state| {
            if !state["config"].is_object() {
                state["config"] = json!({"platform":"mattermost"});
            }
            Ok(())
        })
        .expect("config");
        WorkerState {
            config: cccc_core::im_state::load(&store, group).expect("state")["config"]
                .as_object()
                .expect("config")
                .clone(),
            generation: 1,
            generations: Arc::new(Mutex::new([(group.to_owned(), 1)].into_iter().collect())),
        }
    }

    async fn fixture_for_bot(token: &str, bot_id: &str) -> Fixture {
        async fn ws(
            State(state): State<Arc<MockState>>,
            axum::extract::Query(query): axum::extract::Query<
                std::collections::HashMap<String, String>,
            >,
            headers: HeaderMap,
            upgrade: WebSocketUpgrade,
        ) -> Response {
            assert_eq!(
                headers
                    .get("authorization")
                    .expect("Mattermost test operation"),
                format!("Bearer {}", state.token).as_str()
            );
            let mode = state.ws_mode.load(Ordering::SeqCst);
            state.ws_queries.lock().expect("queries").push(query);
            state.ws_attempts.fetch_add(1, Ordering::SeqCst);
            if let Some(status) = match mode {
                1 => Some(StatusCode::UNAUTHORIZED),
                6 => Some(StatusCode::FORBIDDEN),
                7 => Some(StatusCode::SERVICE_UNAVAILABLE),
                8 => Some(StatusCode::TOO_MANY_REQUESTS),
                _ => None,
            } {
                return status.into_response();
            }
            let connection = state.ws_connections.fetch_add(1, Ordering::SeqCst);
            upgrade.on_upgrade(move |mut socket| async move {
                let text = match mode {
                    2 => json!({"error":{"message":"test authentication rejection"}}).to_string(),
                    4 => "invalid-json".to_owned(),
                    10 if connection > 0 => json!({"event":"hello","data":{"connection_id":"n".repeat(26)},"seq":0}).to_string(),
                    _ => json!({"event":"hello","data":{"connection_id":"s".repeat(26)},"seq":0}).to_string(),
                };
                if !(mode == 9 && connection > 0) {
                    socket
                        .send(axum::extract::ws::Message::Text(text.into()))
                        .await
                        .expect("Mattermost test operation");
                }
                if mode == 9 {
                    // Skip seq=2 after the first connection's seq=1; resume must request next=2 without requiring hello.
                    let sequences: &[u64] = if connection == 0 { &[1, 3] } else { &[1, 2, 2, 3, 4] };
                    for seq in sequences {
                        let event = if *seq == 3 {
                            json!({"event":"status_change","seq":seq})
                        } else {
                            let post = json!({"id":format!("{seq:026}"),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot /help","type":""});
                            json!({"event":"posted","seq":seq,"data":{"channel_type":"O","post":post.to_string()}})
                        };
                        let _ = socket.send(axum::extract::ws::Message::Text(event.to_string().into())).await;
                    }
                }
                if (mode == 3 && connection == 0) || (mode == 10 && connection < 2) {
                    let _ = socket.send(axum::extract::ws::Message::Close(None)).await;
                    return;
                }
                let events = state.ws_events.lock().expect("events").clone();
                for (index, mut event) in events.into_iter().enumerate() {
                    event["seq"] = json!(index + 1);
                    socket.send(axum::extract::ws::Message::Text(event.to_string().into())).await.expect("event");
                }
                let mut heartbeat = tokio::time::interval(Duration::from_millis(30));
                loop {
                    tokio::select! {
                        message = socket.next() => match message {
                            Some(Ok(axum::extract::ws::Message::Ping(data))) => {
                                state.ws_pings.fetch_add(1, Ordering::SeqCst);
                                if socket.send(axum::extract::ws::Message::Pong(data)).await.is_err() { break; }
                            }
                            Some(Ok(axum::extract::ws::Message::Pong(_))) => { state.ws_pongs.fetch_add(1, Ordering::SeqCst); }
                            None | Some(Err(_)) | Some(Ok(axum::extract::ws::Message::Close(_))) => break,
                            _ => {}
                        },
                        _ = heartbeat.tick(), if mode == 5 => {
                            if socket.send(axum::extract::ws::Message::Ping(vec![1].into())).await.is_err() { break; }
                        }
                    }
                }
            })
        }
        async fn http(State(state): State<Arc<MockState>>, request: Request) -> Response {
            if request
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                != Some(format!("Bearer {}", state.token).as_str())
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            let method = request.method().clone();
            let path = request.uri().path().to_owned();
            if path == "/sub/api/v4/test-redirect" {
                return (
                    StatusCode::TEMPORARY_REDIRECT,
                    [("location", "/sub/api/v4/users/me")],
                )
                    .into_response();
            }
            if path == "/sub/api/v4/test-rate" {
                let mut count = state.rate_requests.lock().expect("rate requests");
                *count += 1;
                if *count == 1 {
                    return (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "1")]).into_response();
                }
                return axum::Json(json!({"ok":true})).into_response();
            }
            if path == "/sub/api/v4/users/me" {
                state.identity_requests.fetch_add(1, Ordering::SeqCst);
                if state.blocked_identity.swap(false, Ordering::SeqCst) {
                    state.identity_entered.notify_one();
                    state.release_identity.notified().await;
                }
                return axum::Json(json!({"id":state.bot_id,"username":"cccc_bot","is_bot":true}))
                    .into_response();
            }
            if path.starts_with("/sub/api/v4/users/") && method == Method::GET {
                state.lookup_calls.fetch_add(1, Ordering::SeqCst);
                if state.fail_lookup.load(Ordering::SeqCst) == 2 {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                let id = if state.fail_lookup.load(Ordering::SeqCst) == 3 {
                    Some("identity-mismatch")
                } else {
                    path.rsplit('/').next()
                };
                return axum::Json(
                    json!({"id":id,"is_bot":state.other_user_is_bot.load(Ordering::SeqCst)}),
                )
                .into_response();
            }
            if path.starts_with("/sub/api/v4/channels/") && method == Method::GET {
                state.lookup_calls.fetch_add(1, Ordering::SeqCst);
                if state.fail_lookup.load(Ordering::SeqCst) == 1 {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                return axum::Json(json!({"id":path.rsplit('/').next(),"type":"O"}))
                    .into_response();
            }
            if path.starts_with("/sub/api/v4/files/") && method == Method::GET {
                *state.downloads.lock().expect("downloads") += 1;
                if state.blocked_download.load(Ordering::SeqCst) {
                    state.release_download.notified().await;
                }
                let id = path.split('/').nth(5).expect("file ID");
                let file = state
                    .inbound_files
                    .lock()
                    .expect("test attachment")
                    .get(id)
                    .cloned();
                let Some(file) = file else {
                    return StatusCode::NOT_FOUND.into_response();
                };
                if path.ends_with("/info") {
                    return axum::Json(file).into_response();
                }
                let data = field(&file, "data").as_bytes().to_vec();
                let mode = field(&file, "body_mode").to_owned();
                if mode == "http_error" {
                    return StatusCode::NOT_FOUND.into_response();
                }
                if mode.is_empty() {
                    return data.into_response();
                }
                let body = async_stream::stream! {
                    yield Ok::<_, std::io::Error>(axum::body::Bytes::from(data));
                    state.file_stream_entered.notify_one();
                    if mode == "blocked" {
                        state.release_download.notified().await;
                    } else if mode == "stream_error" {
                        yield Err(std::io::Error::other("test stream interruption"));
                    }
                };
                return axum::body::Body::from_stream(body).into_response();
            }
            let upload_metadata = if path == "/sub/api/v4/files" && method == Method::POST {
                let url = reqwest::Url::parse(&format!("http://localhost{}", request.uri()))
                    .expect("upload request URL");
                let query: std::collections::HashMap<String, String> =
                    url.query_pairs().into_owned().collect();
                let channel = query.get("channel_id").expect("upload channel");
                let filename = query.get("filename").expect("upload filename");
                assert!(valid_id(channel));
                assert!(!filename.is_empty());
                assert_eq!(
                    request
                        .headers()
                        .get("content-type")
                        .expect("upload content type"),
                    "application/octet-stream"
                );
                Some(
                    json!({"channel_id":channel,"filename":filename,"content_type":"application/octet-stream"}),
                )
            } else {
                None
            };
            let raw = axum::body::to_bytes(request.into_body(), 20 * 1024 * 1024)
                .await
                .expect("Mattermost test operation");
            if path == "/sub/api/v4/reactions"
                || (path.contains("/reactions/") && method == Method::DELETE)
            {
                let body = serde_json::from_slice(&raw).unwrap_or(Value::Null);
                state
                    .reactions
                    .lock()
                    .expect("reactions")
                    .push((method, path, body));
                if state.forbidden.load(Ordering::SeqCst) {
                    return StatusCode::FORBIDDEN.into_response();
                }
                return axum::Json(json!({"status":"OK"})).into_response();
            }
            if path == "/sub/api/v4/files" && method == Method::POST {
                state
                    .upload_metadata
                    .lock()
                    .expect("upload metadata")
                    .push(upload_metadata.expect("validated upload metadata"));
                state
                    .uploads
                    .lock()
                    .expect("Mattermost test operation")
                    .push(raw.to_vec());
                if state.forbidden.load(Ordering::SeqCst) {
                    return StatusCode::FORBIDDEN.into_response();
                }
                return axum::Json(json!({"file_infos":[{"id":"f".repeat(26)}]})).into_response();
            }
            if path == "/sub/api/v4/posts" && method == Method::POST {
                if state.fail_create.load(Ordering::Relaxed) {
                    state.failed_posts.fetch_add(1, Ordering::SeqCst);
                    return StatusCode::FORBIDDEN.into_response();
                }
                let value: Value = serde_json::from_slice(&raw).expect("Mattermost test operation");
                let mut posts = state.posts.lock().expect("Mattermost test operation");
                posts.push(value);
                return axum::Json(json!({"id":format!("{:026}", posts.len())})).into_response();
            }
            if path.starts_with("/sub/api/v4/posts/") && path.ends_with("/patch") {
                assert_eq!(method, Method::PUT);
                if state.fail_edit.load(Ordering::Relaxed) {
                    return StatusCode::FORBIDDEN.into_response();
                }
                let id = path.split('/').nth(5).expect("edited post ID");
                let index = id.parse::<usize>().expect("mock post index") - 1;
                let patch: Value = serde_json::from_slice(&raw).expect("edited body");
                state.posts.lock().expect("posts")[index]["message"] = patch["message"].clone();
                return axum::Json(json!({"id":path.split('/').nth(5).unwrap_or_default()}))
                    .into_response();
            }
            StatusCode::NOT_FOUND.into_response()
        }
        let state = Arc::new(MockState {
            token: token.to_owned(),
            bot_id: bot_id.to_owned(),
            ..MockState::default()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mattermost test operation");
        let site = format!(
            "http://{}/sub",
            listener.local_addr().expect("Mattermost test operation")
        );
        let router = Router::new()
            .route("/sub/api/v4/websocket", get(ws))
            .fallback(any(http))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("Mattermost test operation");
        });
        let api = MattermostApi {
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("Mattermost test operation"),
            site,
            token: token.to_owned(),
            bot_id: bot_id.to_owned(),
            username: "cccc_bot".into(),
            worker: None,
        };
        Fixture { api, state, task }
    }

    fn scope() -> (tempfile::TempDir, HomeLayout, String) {
        let temp = tempfile::tempdir().expect("Mattermost test operation");
        let home =
            HomeLayout::from_path(temp.path().join("home")).expect("Mattermost test operation");
        let group = GroupStore::new(home.clone())
            .expect("Mattermost test operation")
            .create("Mattermost", "")
            .expect("Mattermost test operation");
        (temp, home, group.group_id)
    }

    fn authorize_target(home: &HomeLayout, group: &str, thread_id: &str, paused: bool) {
        let store = GroupStore::new(home.clone()).expect("store");
        cccc_core::im_state::update(&store, group, |state| {
            let target = json!([{"platform":"mattermost","chat_id":"c".repeat(26),"thread_id":thread_id,"authorized_at":1,"paused":paused}]);
            state["authorized"] = target.clone();
            state["subscribers"] = target;
            Ok(())
        }).expect("authorize target");
    }

    fn attachment_request(
        fixture: &Fixture,
        home: &HomeLayout,
        group: &str,
    ) -> (MattermostInbound, Value) {
        authorize_target(home, group, "", false);
        let files = [
            ("f", "first", "材料.txt", "text/plain"),
            ("g", "second", "图片.png", "image/png"),
        ];
        *fixture.state.inbound_files.lock().expect("test attachment") = files.into_iter().map(|(id, data, name, mime)| {
            let id = id.repeat(26);
            (id.clone(), json!({"id":id,"post_id":"p".repeat(26),"name":name,"mime_type":mime,"size":data.len(),"data":data}))
        }).collect();
        let config = json!({"files":{"max_mb":1}});
        let inbound = MattermostInbound::new(
            home.clone(),
            group,
            DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(3)),
            fixture.api.clone(),
            MattermostReactions::new(home.clone(), group, fixture.api.clone()),
            config.as_object().expect("config"),
        );
        let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot /send @user See attachments","type":"","file_ids":["f".repeat(26),"g".repeat(26)]});
        (
            inbound,
            json!({"event":"posted","data":{"channel_type":"O","post":post.to_string()}}),
        )
    }

    fn blob_names(home: &HomeLayout, group: &str) -> std::collections::BTreeSet<String> {
        let path = GroupStore::new(home.clone())
            .expect("GroupStore")
            .state_dir(group)
            .expect("state directory")
            .join("blobs");
        std::fs::read_dir(path)
            .expect("Blob directory")
            .map(|entry| {
                entry
                    .expect("Blob file")
                    .file_name()
                    .to_str()
                    .expect("filename")
                    .to_owned()
            })
            .collect()
    }

    #[tokio::test]
    async fn later_attachment_failure_cleans_staging_without_deleting_existing_blobs() {
        for mode in [
            "http_error",
            "source",
            "size",
            "length",
            "chunked",
            "stream_error",
            "metadata_larger",
            "metadata_smaller",
            "invalid_id",
            "existing",
        ] {
            let fixture = fixture().await;
            let (_temp, home, group) = scope();
            let (mut inbound, mut event) = attachment_request(&fixture, &home, &group);
            let existing = (mode == "existing").then(|| {
                cccc_core::blobs::store(&home, &group, b"first")
                    .expect("Blob shared by existing messages")
            });
            {
                let mut files = fixture.state.inbound_files.lock().expect("attachments");
                let second = files.get_mut(&"g".repeat(26)).expect("second attachment");
                match mode {
                    "source" => second["post_id"] = json!("q".repeat(26)),
                    "size" => second["size"] = json!(1024 * 1024 + 1),
                    "metadata_larger" => second["size"] = json!(100),
                    "metadata_smaller" => second["size"] = json!(1),
                    "length" | "chunked" => {
                        second["data"] = json!("x".repeat(1024 * 1024 + 1));
                        if mode == "chunked" {
                            second["body_mode"] = json!(mode);
                        }
                    }
                    "existing" => second["body_mode"] = json!("http_error"),
                    "invalid_id" => {
                        let mut post: Value =
                            serde_json::from_str(field(&event["data"], "post")).expect("posts");
                        post["file_ids"][1] = json!("invalid");
                        event["data"]["post"] = json!(post.to_string());
                    }
                    _ => second["body_mode"] = json!(mode),
                }
            }
            let error = inbound
                .handle(&event)
                .await
                .expect_err("second attachment failure");
            assert!(
                !error.contains("submission outcome is unknown"),
                "{mode}: no daemon submission"
            );
            assert!(!error.contains("test-token"));
            let names = blob_names(&home, &group);
            if mode.starts_with("metadata_") {
                assert!(
                    error.contains("size does not match file metadata"),
                    "{mode}: {error}"
                );
            }
            if let Some(blob) = existing {
                assert_eq!(names, [blob.sha256].into_iter().collect(), "{mode}");
                let path =
                    cccc_core::blobs::resolve(&home, &group, &blob.path).expect("original file");
                assert_eq!(std::fs::read(path).expect("original content"), b"first");
            } else {
                assert!(
                    names.is_empty(),
                    "{mode}: no final or temporary files remain {names:?}"
                );
            }
            let store = GroupStore::new(home).expect("GroupStore");
            let events = cccc_core::ledger::read_all(&store.ledger_path(&group).expect("Ledger"))
                .expect("events");
            assert!(
                !events.iter().any(|event| event.kind == "chat.message"),
                "{mode}"
            );
            let posts = fixture.state.posts.lock().expect("feedback");
            assert_eq!(posts.len(), 1, "{mode}");
            assert!(
                field(&posts[0], "message")
                    .contains("Could not deliver the message or attachments to CCCC"),
                "{mode}"
            );
        }
    }

    #[tokio::test]
    async fn cancelling_later_attachment_removes_pending_uploads() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let (mut inbound, event) = attachment_request(&fixture, &home, &group);
        fixture
            .state
            .inbound_files
            .lock()
            .expect("attachments")
            .get_mut(&"g".repeat(26))
            .expect("second attachment")["body_mode"] = json!("blocked");
        let task = tokio::spawn(async move { inbound.handle(&event).await });
        let entered = tokio::time::timeout(
            Duration::from_secs(5),
            fixture.state.file_stream_entered.notified(),
        )
        .await;
        let pending = blob_names(&home, &group);
        task.abort();
        let cancelled = task.await.expect_err("cancelled task");
        fixture.state.release_download.notify_one();
        entered.expect("second attachment transfer started");
        assert!(cancelled.is_cancelled());
        assert!(!pending.is_empty(), "the first attachment remains staged");
        assert!(
            pending.iter().all(|name| name.len() != 64),
            "no final content-addressed files yet"
        );
        assert!(
            blob_names(&home, &group).is_empty(),
            "native destructors remove all staged files"
        );
        assert!(fixture.state.posts.lock().expect("feedback").is_empty());
        let store = GroupStore::new(home).expect("GroupStore");
        let events = cccc_core::ledger::read_all(&store.ledger_path(&group).expect("Ledger"))
            .expect("events");
        assert!(!events.iter().any(|event| event.kind == "chat.message"));
    }

    #[tokio::test]
    async fn completed_attachments_are_committed_together_with_full_metadata() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        home.initialize().expect("Home");
        let (mut inbound, event) = attachment_request(&fixture, &home, &group);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listen");
        let address = cccc_contracts::DaemonAddress {
            v: 1,
            transport: cccc_contracts::Transport::Tcp,
            path: String::new(),
            host: "127.0.0.1".into(),
            port: listener.local_addr().expect("address").port(),
            pid: std::process::id(),
            version: "test".into(),
            ts: "test".into(),
        };
        std::fs::write(
            home.daemon_dir().join("ccccd.addr.json"),
            serde_json::to_vec(&address).expect("address JSON"),
        )
        .expect("address file");
        let server_home = home.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept request");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.expect("request");
            let request: cccc_contracts::DaemonRequest =
                serde_json::from_str(&line).expect("request JSON");
            let response = cccc_daemon::handle_request(&server_home, &request);
            assert!(response.ok, "{:?}", response.error);
            let mut bytes = serde_json::to_vec(&response).expect("response");
            bytes.push(b'\n');
            stream
                .get_mut()
                .write_all(&bytes)
                .await
                .expect("write response");
        });
        let result = inbound.handle(&event).await;
        if result.is_err() {
            server.abort();
        }
        result.expect("submit both attachments");
        server.await.expect("real daemon request completed");
        inbound
            .handle(&event)
            .await
            .expect("replay must not download or submit again");
        assert_eq!(*fixture.state.downloads.lock().expect("download count"), 4);
        assert_eq!(blob_names(&home, &group).len(), 2, "only two final Blobs");
        let store = GroupStore::new(home.clone()).expect("GroupStore");
        let events = cccc_core::ledger::read_all(&store.ledger_path(&group).expect("Ledger"))
            .expect("events");
        let messages: Vec<_> = events
            .iter()
            .filter(|event| event.kind == "chat.message")
            .collect();
        assert_eq!(messages.len(), 1);
        let attachments = messages[0].data["attachments"]
            .as_array()
            .expect("attachments");
        assert_eq!(attachments.len(), 2);
        for (attachment, (id, data, title, mime, kind)) in attachments.iter().zip([
            ("f", "first", "材料.txt", "text/plain", "file"),
            ("g", "second", "图片.png", "image/png", "image"),
        ]) {
            use sha2::Digest;
            assert_eq!(attachment["source_media_id"], id.repeat(26));
            assert_eq!(attachment["title"], title);
            assert_eq!(attachment["mime_type"], mime);
            assert_eq!(attachment["kind"], kind);
            assert_eq!(attachment["bytes"], data.len());
            assert_eq!(
                attachment["sha256"],
                format!("{:x}", sha2::Sha256::digest(data.as_bytes()))
            );
            let path = cccc_core::blobs::resolve(
                &home,
                &group,
                attachment["path"].as_str().expect("path"),
            )
            .expect("Blob path");
            assert_eq!(std::fs::read(path).expect("content"), data.as_bytes());
        }
    }

    #[tokio::test]
    async fn socket_write_deadline_covers_ping_and_pong_and_can_be_cancelled() {
        // Block the production sender's Sink::poll_flush without relying on kernel buffer sizes.
        for message in [
            Message::Ping(Vec::new().into()),
            Message::Pong(Vec::new().into()),
        ] {
            let mut sink = Box::pin(futures_util::sink::unfold((), |(), _: Message| {
                std::future::pending::<Result<(), std::io::Error>>()
            }));
            let result =
                tokio::time::timeout(Duration::from_secs(7), socket_send(&mut sink, message))
                    .await
                    .expect("bounded write");
            assert_eq!(result, Err("Mattermost WebSocket write timed out"));
        }
        let task = tokio::spawn(async {
            let mut sink = Box::pin(futures_util::sink::unfold((), |(), _: Message| {
                std::future::pending::<Result<(), std::io::Error>>()
            }));
            socket_send(&mut sink, Message::Ping(Vec::new().into())).await
        });
        tokio::task::yield_now().await;
        task.abort();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .expect("cancel write")
                .expect_err("cancelled")
                .is_cancelled()
        );
    }

    #[tokio::test]
    async fn lookup_failure_feedback_respects_addressing_authorization_and_thread() {
        for (failure, authorized, paused, thread, raw, expected) in [
            (2, true, false, "", "@cccc_bot hello", true),
            (1, true, false, "", "@cccc_bot hello", true),
            (2, false, false, "", "@cccc_bot hello", false),
            (2, true, true, "", "@cccc_bot hello", false),
            (
                2,
                true,
                false,
                "tttttttttttttttttttttttttt",
                "@cccc_bot hello",
                false,
            ),
            (2, true, false, "", "Ordinary channel chat", false),
            (
                1,
                true,
                false,
                "",
                "Unknown channel types must not be inferred as DMs",
                false,
            ),
        ] {
            let fixture = fixture().await;
            fixture.state.fail_lookup.store(failure, Ordering::SeqCst);
            let (_temp, home, group) = scope();
            if authorized {
                authorize_target(&home, &group, "", paused);
            }
            let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
            let mut inbound = MattermostInbound::new(
                home.clone(),
                &group,
                DaemonClient::new(home.clone()),
                fixture.api.clone(),
                reactions,
                &Map::new(),
            );
            let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":thread,"message":raw,"file_ids":["f".repeat(26)],"type":""});
            let event = json!({"event":"posted","data":{"channel_type":if failure == 1 { "" } else { "O" },"post":post.to_string()}});
            let store = GroupStore::new(home.clone()).expect("store");
            let before = cccc_core::im_state::load(&store, &group).expect("state");
            let _ = inbound.handle(&event).await;
            let posts = fixture.state.posts.lock().expect("posts");
            assert_eq!(
                posts.len(),
                usize::from(expected),
                "failure={failure}, authorized={authorized}, paused={paused}, thread={thread}, raw={raw}"
            );
            if expected {
                assert!(field(&posts[0], "message").contains("has not been submitted to CCCC"));
                assert_eq!(field(&posts[0], "channel_id"), "c".repeat(26));
            }
            assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
            assert!(
                fixture
                    .state
                    .reactions
                    .lock()
                    .expect("reactions")
                    .is_empty()
            );
            assert_eq!(
                cccc_core::im_state::load(&store, &group).expect("state"),
                before
            );
            assert!(
                cccc_core::ledger::read_all(&store.ledger_path(&group).expect("ledger"))
                    .expect("events")
                    .iter()
                    .all(|event| event.kind != "chat.message")
            );
        }
    }

    #[tokio::test]
    async fn lookup_failure_replays_are_deduplicated_but_new_posts_can_retry() {
        for failure in [1, 2, 3] {
            for reject_reply in [false, true] {
                let fixture = fixture().await;
                fixture.state.fail_lookup.store(failure, Ordering::SeqCst);
                fixture
                    .state
                    .fail_create
                    .store(reject_reply, Ordering::SeqCst);
                let (_temp, home, group) = scope();
                authorize_target(&home, &group, "", false);
                let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
                let mut inbound = MattermostInbound::new(
                    home.clone(),
                    &group,
                    DaemonClient::new(home.clone()),
                    fixture.api.clone(),
                    reactions,
                    &Map::new(),
                );
                let mut post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot hello","file_ids":["f".repeat(26)],"type":""});
                let event = |post: &Value| json!({"event":"posted","data":{"channel_type":if failure == 1 { "" } else { "O" },"post":post.to_string()}});
                inbound
                    .handle(&event(&post))
                    .await
                    .expect_err("first lookup failure");
                inbound
                    .handle(&event(&post))
                    .await
                    .expect("duplicate ignored");
                assert_eq!(fixture.state.lookup_calls.load(Ordering::SeqCst), 1);
                assert_eq!(
                    fixture.state.posts.lock().expect("posts").len(),
                    usize::from(!reject_reply)
                );
                assert_eq!(
                    fixture.state.failed_posts.load(Ordering::SeqCst),
                    usize::from(reject_reply)
                );
                // A new source post can retry; an identity lookup failure must not permanently classify the sender as a Bot.
                post["id"] = json!("q".repeat(26));
                inbound
                    .handle(&event(&post))
                    .await
                    .expect_err("new post retries");
                assert_eq!(fixture.state.lookup_calls.load(Ordering::SeqCst), 2);
                assert_eq!(
                    fixture.state.posts.lock().expect("posts").len(),
                    2 * usize::from(!reject_reply)
                );
                assert_eq!(
                    fixture.state.failed_posts.load(Ordering::SeqCst),
                    2 * usize::from(reject_reply)
                );
                assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
                let store = GroupStore::new(home).expect("store");
                assert!(
                    cccc_core::ledger::read_all(&store.ledger_path(&group).expect("ledger"))
                        .expect("ledger")
                        .iter()
                        .all(|event| event.kind != "chat.message")
                );
            }
        }
    }

    #[tokio::test]
    async fn command_reply_replays_do_not_repeat_decisions_when_feedback_fails() {
        for text in [
            "@cccc_bot /help",
            "@cccc_bot /unsubscribe",
            "@cccc_bot Unauthorized message",
        ] {
            for reject_reply in [false, true] {
                let fixture = fixture().await;
                fixture
                    .state
                    .fail_create
                    .store(reject_reply, Ordering::SeqCst);
                let (_temp, home, group) = scope();
                let unsubscribe = text.ends_with("/unsubscribe");
                if unsubscribe {
                    authorize_target(&home, &group, "", false);
                }
                let mut inbound = MattermostInbound::new(
                    home.clone(),
                    &group,
                    DaemonClient::new(home.clone()),
                    fixture.api.clone(),
                    MattermostReactions::new(home.clone(), &group, fixture.api.clone()),
                    &Map::new(),
                );
                let mut post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":text,"type":""});
                let event = |post: &Value| json!({"event":"posted","data":{"channel_type":"O","post":post.to_string()}});
                assert_eq!(inbound.handle(&event(&post)).await.is_err(), reject_reply);
                if unsubscribe {
                    assert!(authorized_chats(&home, &group, PLATFORM).is_empty());
                    // Reauthorize after the first unsubscribe; replaying the old post must not unsubscribe again.
                    authorize_target(&home, &group, "", false);
                }
                inbound
                    .handle(&event(&post))
                    .await
                    .expect("source post deduplication");
                if unsubscribe {
                    assert_eq!(authorized_chats(&home, &group, PLATFORM).len(), 1);
                }
                assert_eq!(
                    fixture.state.posts.lock().expect("posts").len(),
                    usize::from(!reject_reply)
                );
                assert_eq!(
                    fixture.state.failed_posts.load(Ordering::SeqCst),
                    usize::from(reject_reply)
                );
                post["id"] = json!("q".repeat(26));
                assert_eq!(inbound.handle(&event(&post)).await.is_err(), reject_reply);
                if unsubscribe {
                    assert!(authorized_chats(&home, &group, PLATFORM).is_empty());
                }
                assert_eq!(
                    fixture.state.posts.lock().expect("posts").len(),
                    2 * usize::from(!reject_reply)
                );
                assert_eq!(
                    fixture.state.failed_posts.load(Ordering::SeqCst),
                    2 * usize::from(reject_reply)
                );
                assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
                assert!(
                    fixture
                        .state
                        .reactions
                        .lock()
                        .expect("reactions")
                        .is_empty()
                );
                let store = GroupStore::new(home).expect("store");
                assert!(
                    cccc_core::ledger::read_all(&store.ledger_path(&group).expect("ledger"))
                        .expect("ledger")
                        .iter()
                        .all(|event| event.kind != "chat.message")
                );
            }
        }
    }

    #[tokio::test]
    async fn attachment_failure_replays_do_not_repeat_downloads_or_feedback() {
        for reject_reply in [false, true] {
            let fixture = fixture().await;
            fixture
                .state
                .fail_create
                .store(reject_reply, Ordering::SeqCst);
            let (_temp, home, group) = scope();
            let (mut inbound, mut event) = attachment_request(&fixture, &home, &group);
            fixture
                .state
                .inbound_files
                .lock()
                .expect("files")
                .get_mut(&"g".repeat(26))
                .expect("second attachment")["body_mode"] = json!("http_error");
            inbound
                .handle(&event)
                .await
                .expect_err("attachment failure");
            let downloads = *fixture.state.downloads.lock().expect("downloads");
            let reactions = fixture.state.reactions.lock().expect("reactions").len();
            assert_eq!(downloads, 4);
            inbound
                .handle(&event)
                .await
                .expect("source post deduplication");
            assert_eq!(
                *fixture.state.downloads.lock().expect("downloads"),
                downloads
            );
            assert_eq!(
                fixture.state.reactions.lock().expect("reactions").len(),
                reactions
            );
            assert_eq!(
                fixture.state.posts.lock().expect("posts").len(),
                usize::from(!reject_reply)
            );
            assert_eq!(
                fixture.state.failed_posts.load(Ordering::SeqCst),
                usize::from(reject_reply)
            );
            let mut post: Value =
                serde_json::from_str(field(&event["data"], "post")).expect("posts");
            post["id"] = json!("q".repeat(26));
            event["data"]["post"] = json!(post.to_string());
            for file in fixture
                .state
                .inbound_files
                .lock()
                .expect("files")
                .values_mut()
            {
                file["post_id"] = json!("q".repeat(26));
            }
            inbound
                .handle(&event)
                .await
                .expect_err("retry with a new post");
            assert_eq!(
                *fixture.state.downloads.lock().expect("downloads"),
                2 * downloads
            );
            assert_eq!(
                fixture.state.posts.lock().expect("posts").len(),
                2 * usize::from(!reject_reply)
            );
            assert_eq!(
                fixture.state.failed_posts.load(Ordering::SeqCst),
                2 * usize::from(reject_reply)
            );
            assert!(blob_names(&home, &group).is_empty());
            let store = GroupStore::new(home).expect("store");
            assert!(
                cccc_core::ledger::read_all(&store.ledger_path(&group).expect("ledger"))
                    .expect("ledger")
                    .iter()
                    .all(|event| event.kind != "chat.message")
            );
        }
    }

    #[tokio::test]
    async fn filtered_lookup_failure_does_not_consume_feedback_deduplication() {
        for paused in [false, true] {
            let fixture = fixture().await;
            fixture.state.fail_lookup.store(2, Ordering::SeqCst);
            let (_temp, home, group) = scope();
            if paused {
                authorize_target(&home, &group, "", true);
            }
            let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
            let mut inbound = MattermostInbound::new(
                home.clone(),
                &group,
                DaemonClient::new(home.clone()),
                fixture.api.clone(),
                reactions,
                &Map::new(),
            );
            let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot hello","type":""});
            let event =
                json!({"event":"posted","data":{"channel_type":"O","post":post.to_string()}});
            inbound.handle(&event).await.expect_err("filtered failure");
            assert!(fixture.state.posts.lock().expect("posts").is_empty());
            authorize_target(&home, &group, "", false);
            inbound
                .handle(&event)
                .await
                .expect_err("now eligible for feedback");
            inbound
                .handle(&event)
                .await
                .expect("handled feedback is deduplicated");
            assert_eq!(fixture.state.posts.lock().expect("posts").len(), 1);
            assert_eq!(fixture.state.lookup_calls.load(Ordering::SeqCst), 2);
            assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
        }
    }

    #[tokio::test]
    async fn lookup_error_reply_failure_is_bounded_and_cached_bots_are_ignored() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        authorize_target(&home, &group, "", false);
        let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
        let mut inbound = MattermostInbound::new(
            home.clone(),
            &group,
            DaemonClient::new(home.clone()),
            fixture.api.clone(),
            reactions,
            &Map::new(),
        );
        let mut post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot /help","type":""});
        fixture
            .state
            .other_user_is_bot
            .store(true, Ordering::SeqCst);
        let event = |post: &Value, channel_type: &str| json!({"event":"posted","data":{"channel_type":channel_type,"post":post.to_string()}});
        inbound
            .handle(&event(&post, "O"))
            .await
            .expect("ignore bot");
        fixture.state.fail_lookup.store(1, Ordering::SeqCst);
        inbound
            .handle(&event(&post, ""))
            .await
            .expect("ignore cached bot without lookup");
        post["user_id"] = json!(fixture.api.bot_id);
        inbound
            .handle(&event(&post, ""))
            .await
            .expect("ignore self");
        assert!(fixture.state.posts.lock().expect("posts").is_empty());
        post["user_id"] = json!("v".repeat(26));
        fixture.state.fail_create.store(true, Ordering::SeqCst);
        inbound
            .handle(&event(&post, ""))
            .await
            .expect_err("lookup failure");
        assert_eq!(fixture.state.failed_posts.load(Ordering::SeqCst), 1);
        let path = GroupStore::new(home)
            .expect("store")
            .state_dir(&group)
            .expect("state")
            .join("im_bridge.log");
        assert!(
            std::fs::read_to_string(path)
                .expect("log")
                .contains("lookup_error_reply")
        );
    }

    #[tokio::test]
    async fn daemon_failure_is_private_and_lost_acceptance_is_not_retried() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        for lose_reply in [false, true] {
            let mut fixture = fixture().await;
            let (_temp, home, group) = scope();
            home.initialize().expect("home");
            authorize_target(&home, &group, "", false);
            fixture.api.worker = Some(worker_state(&home, &group));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("listen");
            let address = cccc_contracts::DaemonAddress {
                v: 1,
                transport: cccc_contracts::Transport::Tcp,
                path: String::new(),
                host: "127.0.0.1".into(),
                port: listener.local_addr().expect("address").port(),
                pid: std::process::id(),
                version: "test".into(),
                ts: "test".into(),
            };
            std::fs::write(
                home.daemon_dir().join("ccccd.addr.json"),
                serde_json::to_vec(&address).expect("address JSON"),
            )
            .expect("address file");
            let server_home = home.clone();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await.expect("request");
                let request: cccc_contracts::DaemonRequest =
                    serde_json::from_str(&line).expect("request JSON");
                let response = cccc_daemon::handle_request(&server_home, &request);
                assert_eq!(response.ok, lose_reply, "{:?}", response.error);
                if !lose_reply {
                    assert!(
                        response
                            .error
                            .as_ref()
                            .expect("rejection")
                            .message
                            .contains("synthetic-private-recipient")
                    );
                    let mut bytes = serde_json::to_vec(&response).expect("response");
                    bytes.push(b'\n');
                    stream
                        .get_mut()
                        .write_all(&bytes)
                        .await
                        .expect("response write");
                }
                drop(stream); // Drop the response after a real daemon submission; this is not a rejection.
                assert!(
                    tokio::time::timeout(Duration::from_millis(400), listener.accept())
                        .await
                        .is_err(),
                    "must not resubmit"
                );
            });
            let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
            let mut inbound = MattermostInbound::new(
                home.clone(),
                &group,
                DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(3)),
                fixture.api.clone(),
                reactions,
                &Map::new(),
            );
            let text = if lose_reply {
                "@cccc_bot /send @user synthetic-private-body"
            } else {
                "@cccc_bot /send @synthetic-private-recipient synthetic-private-body"
            };
            let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":text,"type":""});
            let event =
                json!({"event":"posted","data":{"channel_type":"O","post":post.to_string()}});
            let error = inbound.handle(&event).await.expect_err("safe failure");
            fixture.api.log_error(&home, &group, "inbound", &error);
            fixture.api.persist_error(&home, &group, Some(&error));
            inbound
                .handle(&event)
                .await
                .expect("duplicate rejected or uncertain post is ignored");
            server.await.expect("daemon fixture");
            let store = GroupStore::new(home).expect("store");
            let log = std::fs::read_to_string(
                store
                    .state_dir(&group)
                    .expect("state dir")
                    .join("im_bridge.log"),
            )
            .expect("log");
            let state = cccc_core::im_state::load(&store, &group).expect("state");
            let posts = fixture.state.posts.lock().expect("posts");
            assert_eq!(posts.len(), 1);
            for recorded in [
                &error,
                &log,
                &state["last_error"].to_string(),
                &posts[0].to_string(),
            ] {
                assert!(
                    !recorded.contains("synthetic-private"),
                    "private input in error output"
                );
                assert!(
                    !recorded.contains("test-token"),
                    "bot credential in error output"
                );
            }
            assert!(error.contains(&"p".repeat(26)), "safe source ID retained");
            assert!(field(&posts[0], "message").contains(if lose_reply {
                "Cannot confirm"
            } else {
                "rejected this request"
            }));
            let events = cccc_core::ledger::read_all(&store.ledger_path(&group).expect("ledger"))
                .expect("events");
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == "chat.message")
                    .count(),
                usize::from(lose_reply)
            );
            if lose_reply {
                assert!(
                    !fixture
                        .state
                        .reactions
                        .lock()
                        .expect("reactions")
                        .iter()
                        .any(|(_, _, body)| field(body, "emoji_name") == "x")
                );
            }
        }
    }

    #[test]
    fn group_error_log_appends_redacts_and_isolates_without_tracing() {
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        let other = store.create("Other", "").expect("other group");
        let error = "attachment error\nBearer synthetic-secret";
        log_error(&home, &group, "inbound", error, "synthetic-secret");
        log_error(&home, &group, "inbound", error, "synthetic-secret");
        let path = store
            .state_dir(&group)
            .expect("state")
            .join("im_bridge.log");
        let raw = std::fs::read_to_string(&path).expect("log");
        assert!(!raw.contains("synthetic-secret"));
        let rows: Vec<Value> = raw
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON line"))
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["group_id"], group);
        assert_eq!(rows[0]["operation"], "inbound");
        assert_eq!(rows[0]["error"], "attachment error\nBearer [REDACTED]");
        assert!(!rows[0]["ts"].as_str().expect("timestamp").is_empty());
        assert!(
            !store
                .state_dir(&other.group_id)
                .expect("other state")
                .join("im_bridge.log")
                .exists()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                path.metadata().expect("mode").permissions().mode() & 0o777,
                0o600
            );
        }
        let worker = worker_state(&home, &group);
        let api = MattermostApi {
            http: reqwest::Client::new(),
            site: String::new(),
            token: "synthetic-secret".into(),
            bot_id: String::new(),
            username: String::new(),
            worker: Some(worker),
        };
        api.persist_error(&home, &group, None);
        assert_eq!(std::fs::read_to_string(&path).expect("retained log"), raw);
    }

    #[test]
    fn group_error_log_rotates_and_reports_write_failure() {
        let (_temp, home, group) = scope();
        let dir = GroupStore::new(home.clone())
            .expect("store")
            .state_dir(&group)
            .expect("state");
        std::fs::create_dir_all(&dir).expect("directory");
        let path = dir.join("im_bridge.log");
        std::fs::write(&path, vec![b'x'; 1024 * 1024]).expect("full log");
        append_log(&home, &group, "{}").expect("rotate");
        assert_eq!(std::fs::read_to_string(&path).expect("current"), "{}\n");
        assert_eq!(
            dir.join("im_bridge.log.1")
                .metadata()
                .expect("backup")
                .len(),
            1024 * 1024
        );
        std::fs::remove_file(&path).expect("remove fixture");
        std::fs::create_dir(&path).expect("unwritable file fixture");
        assert!(append_log(&home, &group, "{}").is_err());
        log_error(&home, &group, "inbound", "test", ""); // Logging failures must not panic the messaging task.
    }

    fn target(thread: bool) -> AuthorizedChat {
        AuthorizedChat {
            chat_id: "c".repeat(26),
            thread_id: if thread {
                "t".repeat(26)
            } else {
                String::new()
            },
            verbose: false,
        }
    }

    fn stream(group: &str, op: &str, text: &str) -> Event {
        let mut event = Event::new("chat.stream", group);
        event.by = "reviewer".into();
        event.data = json!({"stream_id":"s1","op":op,"text":text})
            .as_object()
            .expect("Mattermost test operation")
            .clone();
        event
    }

    #[tokio::test]
    async fn tls_protocol_errors_are_explicit_and_do_not_echo_credentials() {
        let fixture = fixture().await;
        // Send TLS to a plaintext fixture; never disable certificate validation to make it pass.
        let config = json!({"mattermost_url":fixture.api.site.replacen("http://", "https://", 1),"bot_token":"tls-test-secret"});
        let result = authenticate(config.as_object().expect("config")).await;
        let error = match result {
            Ok(_) => panic!("TLS protocol failure must prevent authentication"),
            Err(error) => error,
        };
        assert!(!error.is_empty());
        assert!(!error.contains("tls-test-secret"));
        assert!(!error.contains(&fixture.api.site));
    }

    #[tokio::test]
    async fn proxy_environment_applies_to_rest_and_websocket() {
        const CASE: &str = "CCCC_MM_PROXY_TEST_CASE";
        if let Ok(case) = std::env::var(CASE) {
            let config = json!({"mattermost_url":std::env::var("CCCC_MM_PROXY_TEST_SITE").expect("site"),"bot_token":"test-token"});
            let result = authenticate(config.as_object().expect("config")).await;
            if case == "rejected" {
                let error = match result {
                    Ok(_) => panic!("an unreachable proxy must prevent authentication"),
                    Err(error) => error,
                };
                assert!(!error.is_empty());
                assert!(!error.contains("test-token"));
                return;
            }
            let api = result.unwrap_or_else(|error| panic!("REST: {error}"));
            let socket = api.socket().await;
            if case == "ws_rejected" {
                let error = socket.expect_err("report the proxied WebSocket rejection");
                assert!(error.to_string().contains("401"));
                assert!(!error.to_string().contains("test-token"));
            } else {
                socket.expect("REST and WebSocket must use the same proxy policy");
            }
            return;
        }
        let fixture = fixture().await;
        for case in ["proxied", "bypass", "rejected", "ws_rejected"] {
            fixture
                .state
                .ws_mode
                .store(usize::from(case == "ws_rejected"), Ordering::SeqCst);
            let mut child =
                tokio::process::Command::new(std::env::current_exe().expect("test executable"));
            child.args([
                "im_runtime::mattermost::tests::proxy_environment_applies_to_rest_and_websocket",
                "--exact",
                "--nocapture",
            ]);
            for name in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "NO_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
                "no_proxy",
            ] {
                child.env_remove(name);
            }
            let proxy = if matches!(case, "bypass" | "rejected") {
                "http://127.0.0.1:0"
            } else {
                fixture.api.site.as_str()
            };
            // The .invalid host is reachable only through the fixture proxy, so a direct connection cannot mask a broken proxy policy.
            let site = if case == "bypass" {
                fixture.api.site.as_str()
            } else {
                "http://cccc-proxy.invalid/sub"
            };
            child
                .env(CASE, case)
                .env("CCCC_MM_PROXY_TEST_SITE", site)
                .env("HTTP_PROXY", proxy)
                .env("HTTPS_PROXY", proxy)
                .env("ALL_PROXY", proxy)
                .env("NO_PROXY", if case == "bypass" { "127.0.0.1" } else { "" });
            let output =
                tokio::time::timeout(Duration::from_secs(20), child.kill_on_drop(true).output())
                    .await
                    .expect("subprocess timeout")
                    .expect("subprocess execution");
            assert!(
                output.status.success(),
                "{case}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[tokio::test]
    async fn forbidden_auxiliary_requests_log_errors_without_losing_text() {
        let fixture = fixture().await;
        fixture.state.forbidden.store(true, Ordering::SeqCst);
        let (_temp, home, group) = scope();
        let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
        let target = target(true);
        let key = target.key();
        reactions.start(&key, &"p".repeat(26)).await;
        reactions.bind(&key, &"p".repeat(26), "event".into());
        assert_eq!(reactions.active.len(&key), 1);
        let blob = cccc_core::blobs::store(&home, &group, b"synthetic-file").expect("blob");
        let sender =
            MattermostOutbound::new(home.clone(), &group, fixture.api.clone(), &Map::new());
        let mut event = Event::new("chat.message", &group);
        event.by = "reviewer".into();
        event.data.insert(
            "text".into(),
            json!("Reaction failures must not discard the body"),
        );
        event.data.insert(
            "attachments".into(),
            json!([{"path":blob.path,"title":"test.txt"}]),
        );
        assert!(
            sender
                .send_target(&target, &event)
                .await
                .expect_err("attachment failure")
                .contains("attachments")
        );
        reactions.complete(&key, Some("event"), false).await;
        assert_eq!(reactions.active.len(&key), 0);
        let posts = fixture.state.posts.lock().expect("posts");
        assert_eq!(posts.len(), 2);
        assert_eq!(
            posts[0]["message"],
            "**reviewer**\n\nReaction failures must not discard the body"
        );
        assert!(field(&posts[1], "message").contains("Some attachments could not be sent"));
        assert!(
            posts
                .iter()
                .all(|p| p["root_id"] == target.thread_id && p["file_ids"] == json!([]))
        );
        let log = std::fs::read_to_string(
            GroupStore::new(home)
                .expect("store")
                .state_dir(&group)
                .expect("state")
                .join("im_bridge.log"),
        )
        .expect("log");
        let operations: Vec<String> = log
            .lines()
            .map(|line| {
                let row: Value = serde_json::from_str(line).expect("JSON");
                assert!(field(&row, "error").contains("403"));
                field(&row, "operation").to_owned()
            })
            .collect();
        assert_eq!(
            operations,
            [
                "reaction_start",
                "upload",
                "reaction_cleanup",
                "reaction_finish"
            ]
        );
        assert!(!log.contains("test-token"));
        assert_eq!(fixture.state.uploads.lock().expect("uploads").len(), 1);
        assert_eq!(fixture.state.reactions.lock().expect("reactions").len(), 3);
    }

    #[tokio::test]
    async fn empty_posts_are_ignored_but_attachment_only_requests_still_require_authorization() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let mut inbound = MattermostInbound::new(
            home.clone(),
            &group,
            DaemonClient::new(home.clone()),
            fixture.api.clone(),
            MattermostReactions::new(home, &group, fixture.api.clone()),
            &Map::new(),
        );
        for channel in ["O", "P", "D", "G"] {
            for text in [
                "",
                " \t\n ",
                "@cccc_bot",
                "@cccc_bot \t\n",
                "@cccc_bot @cccc_bot: ",
            ] {
                let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":text,"type":"","file_ids":[]});
                let event = json!({"event":"posted","data":{"channel_type":channel,"post":post.to_string()}});
                inbound.handle(&event).await.expect("empty post ignored");
            }
        }
        assert!(fixture.state.posts.lock().expect("posts").is_empty());
        assert!(
            fixture
                .state
                .reactions
                .lock()
                .expect("reactions")
                .is_empty()
        );
        assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);

        for (index, channel) in ["O", "P", "D", "G"].iter().enumerate() {
            let text = if matches!(*channel, "D" | "G") {
                ""
            } else {
                "@cccc_bot"
            };
            let post = json!({"id":format!("{}{}", "p".repeat(25), index),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":text,"type":"","file_ids":["f".repeat(26)]});
            let event =
                json!({"event":"posted","data":{"channel_type":channel,"post":post.to_string()}});
            inbound
                .handle(&event)
                .await
                .expect("attachment request checks authorization");
        }
        assert_eq!(fixture.state.posts.lock().expect("posts").len(), 4);
        assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
    }

    #[tokio::test]
    async fn edited_unaddressed_and_bot_posts_are_ignored_and_duplicates_do_not_reply_twice() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let mut inbound = MattermostInbound::new(
            home.clone(),
            &group,
            DaemonClient::new(home.clone()),
            fixture.api.clone(),
            MattermostReactions::new(home, &group, fixture.api.clone()),
            &Map::new(),
        );
        let mut post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot /help","type":""});
        let wrap = |kind: &str, post: &Value| json!({"event":kind,"data":{"channel_type":"O","post":post.to_string()}});
        inbound
            .handle(&wrap("post_edited", &post))
            .await
            .expect("edit ignored");
        post["message"] = json!("Ordinary chat, no Bot response needed");
        inbound
            .handle(&wrap("posted", &post))
            .await
            .expect("ambient ignored");
        post["message"] = json!("@cccc_bot /help");
        fixture
            .state
            .other_user_is_bot
            .store(true, Ordering::SeqCst);
        inbound
            .handle(&wrap("posted", &post))
            .await
            .expect("other bot ignored");
        assert!(fixture.state.posts.lock().expect("posts").is_empty());
        fixture
            .state
            .other_user_is_bot
            .store(false, Ordering::SeqCst);
        post["user_id"] = json!("v".repeat(26));
        inbound
            .handle(&wrap("posted", &post))
            .await
            .expect("human help");
        inbound
            .handle(&wrap("posted", &post))
            .await
            .expect("duplicate ignored");
        assert_eq!(fixture.state.posts.lock().expect("posts").len(), 1);
        assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
    }

    #[tokio::test]
    async fn foreign_group_blobs_cannot_be_uploaded_and_failed_posts_are_not_retried() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let other = GroupStore::new(home.clone())
            .expect("store")
            .create("Other", "")
            .expect("other");
        let blob =
            cccc_core::blobs::store(&home, &other.group_id, b"private-other-group").expect("blob");
        let sender = MattermostOutbound::new(home, &group, fixture.api.clone(), &Map::new());
        let mut event = Event::new("chat.message", &group);
        event.by = "reviewer".into();
        event
            .data
            .insert("text".into(), json!("Only this Group's body"));
        event.data.insert(
            "attachments".into(),
            json!([{"path":blob.path,"title":"test.txt"}]),
        );
        assert!(sender.send_target(&target(false), &event).await.is_err());
        assert!(fixture.state.uploads.lock().expect("uploads").is_empty());
        assert_eq!(
            fixture.state.posts.lock().expect("posts")[0]["message"],
            "**reviewer**\n\nOnly this Group's body"
        );
        fixture.state.fail_create.store(true, Ordering::SeqCst);
        assert!(
            fixture
                .api
                .post(&"c".repeat(26), "", "Do not create a duplicate", &[])
                .await
                .expect_err("403")
                .contains("403")
        );
        assert_eq!(fixture.state.failed_posts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn separate_groups_keep_bot_credentials_authorizations_and_output_isolated() {
        use super::super::authorized_chats;
        let first = fixture_for_bot("synthetic-first-token", &"b".repeat(26)).await;
        let second = fixture_for_bot("synthetic-second-token", &"d".repeat(26)).await;
        let (_temp, home, group1) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        let group2 = store.create("Second", "").expect("group").group_id;
        let chat = "c".repeat(26);
        for (fixture, group, verbose) in [(&first, &group1, false), (&second, &group2, true)] {
            let config = json!({"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":fixture.api.token});
            let item = json!({"platform":"mattermost","chat_id":chat,"verbose":verbose});
            cccc_core::im_state::update(&store, group, |state| {
                *state = json!({"config":config,"authorized":[item],"subscribers":[item]});
                Ok(())
            })
            .expect("config");
            let saved = cccc_core::im_state::load(&store, group).expect("saved");
            let api = authenticate(saved["config"].as_object().expect("config"))
                .await
                .expect("identity");
            assert_eq!(api.bot_id, fixture.api.bot_id);
            api.socket().await.expect("WS identity");
            let targets = authorized_chats(&home, group, PLATFORM);
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].verbose, verbose);
            let sender = MattermostOutbound::new(home.clone(), group, api, &Map::new());
            let mut event = Event::new("chat.message", group);
            event.by = "reviewer".into();
            event.data.insert("text".into(), json!(group));
            sender.send_target(&targets[0], &event).await.expect("send");
        }
        for (fixture, group) in [(&first, &group1), (&second, &group2)] {
            let posts = fixture.state.posts.lock().expect("posts");
            assert_eq!(posts.len(), 1);
            assert_eq!(posts[0]["message"], format!("**reviewer**\n\n{group}"));
        }
        let before_second = cccc_core::im_state::load(&store, &group2).expect("second");
        cccc_core::im_state::update(&store, &group1, |state| {
            state["authorized"] = json!([]);
            state["subscribers"] = json!([]);
            Ok(())
        })
        .expect("revoke first");
        assert!(authorized_chats(&home, &group1, PLATFORM).is_empty());
        assert_eq!(authorized_chats(&home, &group2, PLATFORM).len(), 1);
        assert_eq!(
            cccc_core::im_state::load(&store, &group2).expect("second retained"),
            before_second
        );
        let wrong = json!({"mattermost_url":first.api.site,"bot_token":second.api.token});
        let result = authenticate(wrong.as_object().expect("config")).await;
        assert!(result.err().expect("wrong bot credential").contains("401"));
    }

    #[tokio::test]
    async fn lost_post_response_is_not_retried() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let site = format!("http://{}", listener.local_addr().expect("address"));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let received = bodies.clone();
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.expect("accept");
                let mut reader = BufReader::new(socket);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).await.expect("header") > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().expect("length");
                    }
                }
                let mut body = vec![0; length];
                reader
                    .read_exact(&mut body)
                    .await
                    .expect("complete request");
                received.lock().expect("bodies").push(body);
                // The server received the complete create request but dropped the response; do not create a duplicate.
                drop(reader);
            }
        });
        let api = MattermostApi {
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("client"),
            site,
            token: "synthetic-token".into(),
            bot_id: "b".repeat(26),
            username: "cccc_bot".into(),
            worker: None,
        };
        let result = api.post(&"c".repeat(26), "", "Create once", &[]).await;
        server.abort();
        let _ = server.await;
        let error = result.expect_err("response loss must be reported");
        assert!(!error.contains("synthetic-token"));
        let bodies = bodies.lock().expect("bodies");
        assert_eq!(bodies.len(), 1);
        let body: Value = serde_json::from_slice(&bodies[0]).expect("JSON");
        assert_eq!(body["message"], "Create once");
    }

    #[tokio::test]
    #[ignore = "Requires an authorized test channel; leaves a synthetic long-code thread and makes no model calls"]
    async fn live_long_code_is_losslessly_reassembled() {
        let site = std::env::var("CCCC_MM_TEST_SITE").expect("test site");
        let channel = std::env::var("CCCC_MM_TEST_CHANNEL").expect("test channel");
        assert!(valid_id(&channel));
        let token = std::fs::read_to_string(
            std::env::var("CCCC_MM_TEST_TOKEN_FILE").expect("credential file"),
        )
        .expect("credential");
        let config = json!({"mattermost_url":site,"bot_token":token.trim()});
        let api = authenticate(config.as_object().expect("config"))
            .await
            .expect("api");
        let (_temp, home, group) = scope();
        let root = api
            .post(
                &channel,
                "",
                &format!("Long-code chunking check {group} (synthetic protocol, not model output)"),
                &[],
            )
            .await
            .expect("root");
        let text = format!(
            "```rust\n{}\n```\n[Example link](https://example.com)\nEnd🙂",
            "println!(\"中文🙂\");\n".repeat(1300)
        );
        let mut event = Event::new("chat.message", &group);
        event.by = "reviewer".into();
        event.data.insert("sender_title".into(), json!(" "));
        event.data.insert("text".into(), json!(text));
        let sender = MattermostOutbound::new(home, &group, api.clone(), &Map::new());
        sender
            .send_target(
                &AuthorizedChat {
                    chat_id: channel,
                    thread_id: root.clone(),
                    verbose: false,
                },
                &event,
            )
            .await
            .expect("send");
        let posts = api
            .json(Method::GET, &format!("posts/{root}/thread"), None)
            .await
            .expect("thread");
        let ids: Vec<&str> = posts["order"]
            .as_array()
            .expect("order")
            .iter()
            .rev()
            .filter_map(Value::as_str)
            .filter(|id| *id != root)
            .collect();
        assert!(ids.len() > 1);
        let mut combined = String::new();
        for id in &ids {
            let post = &posts["posts"][*id];
            assert_eq!(post["root_id"], root);
            assert_eq!(post["user_id"], api.bot_id);
            assert!(field(post, "message").chars().count() <= 16_383);
            combined.push_str(field(post, "message"));
        }
        assert_eq!(combined, format!("**reviewer**\n\n{text}"));
        println!(
            "Long-code check root={root} chunks={} chars={} posts={ids:?}",
            ids.len(),
            combined.chars().count()
        );
    }

    #[tokio::test]
    async fn websocket_authentication_failures_are_explicit() {
        let fixture = fixture().await;
        for (mode, expected) in [
            (1, "HTTP 401"),
            (2, "authentication failed"),
            (4, "Invalid Mattermost WebSocket JSON"),
        ] {
            fixture.state.ws_mode.store(mode, Ordering::SeqCst);
            let error = fixture
                .api
                .socket()
                .await
                .expect_err("connection must be rejected")
                .to_string();
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("test-token"));
        }
    }

    fn route_app(home: &HomeLayout) -> Router {
        route_context(home).0
    }

    fn route_context(home: &HomeLayout) -> (Router, Arc<super::super::ImWorkerRegistry>) {
        cccc_core::access_tokens::AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create(
                "test",
                Vec::new(),
                true,
                Some("mattermost-route-test-admin"),
            )
            .expect("test token");
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (app, registry, _, _) = crate::app_with_shutdown(
            home.clone(),
            shutdown,
            crate::WebMode::from_env(),
            None,
            crate::LiveBinding::from_env(),
            crate::new_web_runtime_id(),
        );
        (app, registry)
    }

    async fn route_request(app: Router, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let request = axum::http::Request::builder()
            .method(if body.is_some() {
                Method::POST
            } else {
                Method::GET
            })
            .uri(path)
            .header("authorization", "Bearer mattermost-route-test-admin")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                body.map_or(String::new(), |body| body.to_string()),
            ))
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).expect("JSON response"),
        )
    }

    #[tokio::test]
    async fn delayed_stop_save_and_unset_cannot_overwrite_a_new_start() {
        for action in [
            "stop",
            "same-save",
            "save",
            "other-platform",
            "from-other-platform",
            "unset",
        ] {
            for fail_start in [false, true] {
                let fixture = fixture().await;
                let (_temp, home, group) = scope();
                let (app, registry) = route_context(&home);
                let config = json!({"group_id":group,"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"});
                let slack = json!({"group_id":group,"platform":"slack","bot_token":"test-token","app_token":"test-app-token"});
                let initial = if action == "from-other-platform" {
                    slack.clone()
                } else {
                    config.clone()
                };
                assert_eq!(
                    route_request(app.clone(), "/api/im/set", Some(initial))
                        .await
                        .0,
                    StatusCode::OK
                );
                let store = GroupStore::new(home.clone()).expect("store");
                cccc_core::im_state::update(&store, &group, |value| {
                    value["enabled"] = json!(true);
                    value["running"] = json!(true);
                    value["adapter_available"] = json!(true);
                    value["pid"] = json!(123);
                    value["last_error"] = json!("old worker error");
                    Ok(())
                })
                .expect("running state");
                let entered = Arc::new(tokio::sync::Notify::new());
                let release = Arc::new(tokio::sync::Notify::new());
                let child_release = Arc::clone(&release);
                let stopping = Arc::clone(&entered);
                let worker = super::super::worker(
                    vec![tokio::spawn(async move {
                        child_release.notified().await;
                    })],
                    Arc::new(move || stopping.notify_one()),
                );
                let (generation, previous) = registry.begin_start(&group).await;
                assert!(previous.is_none());
                registry
                    .install(&group, generation, worker)
                    .await
                    .expect("old worker");
                let mut replacement = config.clone();
                let path = match action {
                    "stop" => "/api/im/stop",
                    "unset" => "/api/im/unset",
                    "other-platform" => {
                        replacement = slack;
                        "/api/im/set"
                    }
                    "save" => {
                        replacement["files"] = json!({"max_mb":9});
                        "/api/im/set"
                    }
                    _ => "/api/im/set",
                };
                let old = tokio::spawn(route_request(app.clone(), path, Some(replacement)));
                tokio::time::timeout(Duration::from_secs(3), entered.notified())
                    .await
                    .expect("old shutdown entered");
                let stopped = cccc_core::im_state::load(&store, &group).expect("stopped state");
                assert!(!stopped["running"].as_bool().unwrap_or(false));
                assert!(!stopped["adapter_available"].as_bool().unwrap_or(false));
                assert!(stopped["pid"].is_null());
                assert!(stopped["last_error"].is_null());
                assert!(!registry.is_running(&group));
                if matches!(action, "other-platform" | "unset") {
                    assert_eq!(
                        route_request(app.clone(), "/api/im/set", Some(config))
                            .await
                            .0,
                        StatusCode::OK
                    );
                }
                if fail_start {
                    fixture.state.ws_mode.store(1, Ordering::SeqCst);
                }
                let result = route_request(
                    app.clone(),
                    "/api/im/start",
                    Some(json!({"group_id":group})),
                )
                .await;
                assert_eq!(result.0.is_success(), !fail_start, "{action}");
                let expected = cccc_core::im_state::load(&store, &group).expect("new state");
                assert_eq!(registry.is_running(&group), !fail_start);
                if fail_start {
                    assert!(expected["last_error"].is_string());
                }
                assert!(!old.is_finished(), "old shutdown must still be blocked");
                release.notify_one();
                let completed = old.await.expect("old request");
                assert_eq!(completed.0, StatusCode::OK);
                if action == "stop" {
                    for key in [
                        "enabled",
                        "running",
                        "adapter_available",
                        "pid",
                        "last_error",
                    ] {
                        assert_eq!(completed.1["result"][key], expected[key]);
                    }
                } else if action == "unset" {
                    assert_eq!(completed.1["result"]["configured"], true);
                }
                assert_eq!(
                    cccc_core::im_state::load(&store, &group).expect("final state"),
                    expected,
                    "{action}: fail={fail_start}"
                );
                assert_eq!(registry.is_running(&group), !fail_start);
                registry.shutdown().await;
            }
        }
    }

    #[tokio::test]
    async fn identical_save_invalidates_manual_and_restore_before_allocation() {
        for restoring in [false, true] {
            for action in ["set", "stop"] {
                let fixture = fixture().await;
                let (_temp, home, group) = scope();
                let (app, registry) = route_context(&home);
                let body = json!({"group_id":group,"platform":"mattermost",
                    "mattermost_url":fixture.api.site,"bot_token":"test-token"});
                assert_eq!(
                    route_request(app.clone(), "/api/im/set", Some(body.clone()))
                        .await
                        .0,
                    StatusCode::OK
                );
                let store = GroupStore::new(home.clone()).expect("store");
                cccc_core::im_state::update(&store, &group, |state| {
                    state["enabled"] = json!(true);
                    Ok(())
                })
                .expect("enable restore");
                let (snapshot, version) = super::super::restore_config(&home, &group, &registry)
                    .expect("locked restore snapshot");
                let lock = registry.lifecycle_lock(&group);
                let guard = lock.lock().await;
                let mut starting = Box::pin(async {
                    if restoring {
                        registry
                            .start_with_mode(
                                home.clone(),
                                DaemonClient::new(home.clone()),
                                &group,
                                &snapshot,
                                true,
                                version,
                            )
                            .await
                    } else {
                        let (status, response) = route_request(
                            app.clone(),
                            "/api/im/start",
                            Some(json!({"group_id":group})),
                        )
                        .await;
                        if status.is_success() {
                            Ok(())
                        } else {
                            Err(response.to_string())
                        }
                    }
                });
                assert!(futures_util::poll!(&mut starting).is_pending());
                let path = format!("/api/im/{action}");
                let mut superseding = Box::pin(route_request(app.clone(), &path, Some(body)));
                assert!(futures_util::poll!(&mut superseding).is_pending());
                let before = cccc_core::im_state::load(&store, &group).expect("saved state");
                assert_eq!(before["config"], json!(snapshot));
                assert_ne!(registry.request_version(&group), version);
                drop(guard);
                let (started, superseded) = tokio::time::timeout(Duration::from_secs(10), async {
                    tokio::join!(starting, superseding)
                })
                .await
                .expect("both management requests complete");
                assert!(started.expect_err("stale start").contains("superseded"));
                assert_eq!(superseded.0, StatusCode::OK);
                assert_eq!(
                    cccc_core::im_state::load(&store, &group).expect("unchanged"),
                    before
                );
                assert!(!registry.is_running(&group));
                assert!(
                    !registry
                        .generations
                        .lock()
                        .expect("generations")
                        .contains_key(&group)
                );
                assert_eq!(fixture.state.identity_requests.load(Ordering::SeqCst), 0);
                assert_eq!(fixture.state.ws_attempts.load(Ordering::SeqCst), 0);
                // A fresh request can still start; rejecting a stale snapshot must not disable the current config.
                assert_eq!(
                    route_request(app, "/api/im/start", Some(json!({"group_id":group})))
                        .await
                        .0,
                    StatusCode::OK
                );
                assert!(registry.is_running(&group));
                assert_eq!(fixture.state.identity_requests.load(Ordering::SeqCst), 1);
                registry.shutdown().await;
            }
        }
    }

    #[tokio::test]
    async fn stopped_restore_snapshot_cannot_allocate_a_new_generation() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let (app, registry) = route_context(&home);
        assert_eq!(route_request(app.clone(), "/api/im/set", Some(json!({
            "group_id":group,"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"
        }))).await.0, StatusCode::OK);
        let store = GroupStore::new(home.clone()).expect("store");
        cccc_core::im_state::update(&store, &group, |state| {
            state["enabled"] = json!(true);
            Ok(())
        })
        .expect("enabled");
        let (snapshot, version) =
            super::super::restore_config(&home, &group, &registry).expect("restore snapshot");
        let lock = registry.lifecycle_lock(&group);
        let guard = lock.lock().await;
        // Block the restore path's actual startup function after reading the snapshot and before allocating a generation.
        let mut restoring = Box::pin(registry.start_with_mode(
            home.clone(),
            DaemonClient::new(home.clone()),
            &group,
            &snapshot,
            true,
            version,
        ));
        assert!(futures_util::poll!(&mut restoring).is_pending());
        let stopping = tokio::spawn(route_request(
            app,
            "/api/im/stop",
            Some(json!({"group_id":group})),
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            while cccc_core::im_state::load(&store, &group).expect("state")["enabled"] == true {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("stop saved before generation allocation");
        assert!(!stopping.is_finished());
        let expected = cccc_core::im_state::load(&store, &group).expect("stopped state");
        drop(guard);
        assert!(
            restoring
                .await
                .expect_err("stale restore")
                .contains("superseded")
        );
        assert_eq!(stopping.await.expect("stop").0, StatusCode::OK);
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("final state"),
            expected
        );
        assert!(
            !registry
                .generations
                .lock()
                .expect("generations")
                .contains_key(&group)
        );
        assert!(!registry.is_running(&group));
        assert_eq!(fixture.state.ws_attempts.load(Ordering::SeqCst), 0);
        registry.shutdown().await;
    }

    #[tokio::test]
    async fn legacy_management_cannot_remove_a_new_mattermost_worker() {
        for action in ["stop", "unset", "set"] {
            for replacement in ["mattermost", "slack"] {
                let (_temp, home, group) = scope();
                let (app, registry) = route_context(&home);
                let store = GroupStore::new(home.clone()).expect("store");
                let body = json!({"group_id":group,"platform":"slack",
                    "bot_token":"test-token","app_token":"test-app-token"});
                assert_eq!(
                    route_request(app.clone(), "/api/im/set", Some(body.clone()))
                        .await
                        .0,
                    StatusCode::OK
                );
                let lock = registry.lifecycle_lock(&group);
                let guard = lock.lock().await;
                let path = format!("/api/im/{action}");
                let mut old = Box::pin(route_request(app, &path, Some(body)));
                assert!(futures_util::poll!(&mut old).is_pending());
                // The old request has reached the lifecycle lock; the lock holder commits the new config and installed worker.
                cccc_core::im_state::update(&store, &group, |state| {
                    state["config"]["platform"] = json!(replacement);
                    if replacement == "mattermost" {
                        registry.invalidate_start(&group);
                    }
                    state["running"] = json!(true);
                    state["enabled"] = json!(true);
                    state["adapter_available"] = json!(true);
                    Ok(())
                })
                .expect("replacement");
                let (generation, _) = registry.begin_start_locked(&group);
                let stopped = Arc::new(AtomicBool::new(false));
                let stopped_flag = stopped.clone();
                registry.workers.lock().expect("workers").insert(
                    group.clone(),
                    super::super::worker(
                        vec![tokio::spawn(std::future::pending())],
                        Arc::new(move || {
                            stopped_flag.store(true, Ordering::SeqCst);
                        }),
                    ),
                );
                let before = cccc_core::im_state::load(&store, &group).expect("before");
                drop(guard);
                assert_eq!(old.await.0, StatusCode::OK);
                if replacement == "mattermost" {
                    assert!(!stopped.load(Ordering::SeqCst), "{action}");
                    assert!(registry.is_generation_current(&group, generation));
                    assert!(registry.is_running(&group));
                    assert_eq!(
                        cccc_core::im_state::load(&store, &group).expect("after"),
                        before
                    );
                } else {
                    assert!(stopped.load(Ordering::SeqCst), "legacy {action}");
                    assert!(!registry.is_running(&group));
                }
                registry.shutdown().await;
            }
        }
    }

    #[tokio::test]
    async fn legacy_snapshot_cannot_allocate_after_mattermost_save_before_shutdown() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let (app, registry) = route_context(&home);
        assert_eq!(route_request(app.clone(), "/api/im/set", Some(json!({
            "group_id":group,"platform":"slack","bot_token":"test-token","app_token":"test-app-token"
        }))).await.0, StatusCode::OK);
        let store = GroupStore::new(home.clone()).expect("store");
        let snapshot = cccc_core::im_state::load(&store, &group).expect("snapshot")["config"]
            .as_object()
            .expect("config")
            .clone();
        let (generation, _) = registry.begin_start(&group).await;
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_flag = Arc::clone(&stopped);
        registry
            .install(
                &group,
                generation,
                super::super::worker(
                    vec![tokio::spawn(std::future::pending())],
                    Arc::new(move || {
                        stopped_flag.store(true, Ordering::SeqCst);
                    }),
                ),
            )
            .await
            .expect("old worker");
        let lock = registry.lifecycle_lock(&group);
        let guard = lock.lock().await;
        // Exercise the actual startup allocation entry point; rejection must precede any external Slack request.
        let mut starting = Box::pin(registry.begin_configured_start(
            &home,
            &group,
            &snapshot,
            false,
            (None, None),
        ));
        assert!(futures_util::poll!(&mut starting).is_pending());
        let saving = tokio::spawn(route_request(
            app,
            "/api/im/set",
            Some(json!({
                "group_id":group,"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"
            })),
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            while cccc_core::im_state::load(&store, &group).expect("state")["config"]["platform"]
                != "mattermost"
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("new config saved before shutdown");
        assert!(!saving.is_finished());
        drop(guard);
        assert!(matches!(starting.await, Err(error) if error.contains("superseded")));
        assert_eq!(saving.await.expect("save").0, StatusCode::OK);
        assert!(stopped.load(Ordering::SeqCst));
        assert!(!registry.is_running(&group));
        assert!(
            !registry
                .generations
                .lock()
                .expect("generation")
                .contains_key(&group)
        );
        assert_eq!(fixture.state.ws_attempts.load(Ordering::SeqCst), 0);
        registry.shutdown().await;
    }

    #[tokio::test]
    async fn legacy_start_allocation_keeps_native_snapshot_and_read_failure_behavior() {
        let (_temp, home, group) = scope();
        let (_, registry) = route_context(&home);
        let store = GroupStore::new(home.clone()).expect("store");
        cccc_core::im_state::update(&store, &group, |state| {
            *state =
                json!({"config":{"platform":"discord","bot_token":"test-token"},"enabled":false});
            Ok(())
        })
        .expect("legacy config");
        let before = cccc_core::im_state::load(&store, &group).expect("before");
        let document = store.load(&group).expect("group");
        let snapshot = json!({"platform":"slack","bot_token":"old-token"});
        let (generation, previous) = registry
            .begin_configured_start(
                &home,
                &group,
                snapshot.as_object().expect("config"),
                true,
                (None, None),
            )
            .await
            .expect("legacy snapshot is unchanged");
        assert!(previous.is_none());
        assert!(registry.is_generation_current(&group, generation));
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("after"),
            before
        );
        assert_eq!(store.load(&group).expect("unchanged group"), document);
        assert!(
            registry
                .begin_configured_start(
                    &home,
                    "g_missing",
                    snapshot.as_object().expect("config"),
                    false,
                    (None, None)
                )
                .await
                .is_ok()
        );
        let managed = json!({"platform":"mattermost"});
        assert!(
            registry
                .begin_configured_start(
                    &home,
                    "g_missing",
                    managed.as_object().expect("config"),
                    false,
                    (None, None)
                )
                .await
                .is_err()
        );
        registry.shutdown().await;
    }

    #[tokio::test]
    async fn invalidated_stop_does_not_remove_a_new_generation_or_its_worker() {
        let (_temp, home, group) = scope();
        let (_, registry) = route_context(&home);
        let (old_generation, _) = registry.begin_start(&group).await;
        registry.invalidate_start(&group);
        let (generation, previous) = registry.begin_start(&group).await;
        assert_ne!(generation, old_generation);
        assert!(previous.is_none());
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stopped);
        registry
            .install(
                &group,
                generation,
                super::super::worker(
                    vec![tokio::spawn(std::future::pending())],
                    Arc::new(move || {
                        stop_flag.store(true, Ordering::SeqCst);
                    }),
                ),
            )
            .await
            .expect("new worker");
        registry.stop_invalidated(&group).await;
        assert!(registry.is_generation_current(&group, generation));
        assert!(registry.is_running(&group));
        assert!(!stopped.load(Ordering::SeqCst));
        registry.stop(&group).await;
        assert!(stopped.load(Ordering::SeqCst));
        assert!(!registry.is_running(&group));
    }

    #[tokio::test]
    async fn manual_stop_clears_mattermost_availability_without_changing_legacy_fields() {
        for platform in ["mattermost", "slack"] {
            let fixture = fixture().await;
            let (_temp, home, group) = scope();
            let (app, registry) = route_context(&home);
            assert_eq!(
                route_request(
                    app.clone(),
                    "/api/im/set",
                    Some(json!({
                        "group_id":group,"platform":platform,"mattermost_url":fixture.api.site,
                        "bot_token":"test-token","app_token":"test-app-token"
                    }))
                )
                .await
                .0,
                StatusCode::OK
            );
            let store = GroupStore::new(home).expect("store");
            cccc_core::im_state::update(&store, &group, |value| {
                value["adapter_available"] = json!(true);
                value["pid"] = json!(123);
                value["last_error"] = json!("old error");
                Ok(())
            })
            .expect("old state");
            assert_eq!(
                route_request(app, "/api/im/stop", Some(json!({"group_id":group})))
                    .await
                    .0,
                StatusCode::OK
            );
            let value = cccc_core::im_state::load(&store, &group).expect("state");
            assert_eq!(value["adapter_available"], json!(platform == "slack"));
            assert_eq!(value["running"], json!(false));
            assert_eq!(value["enabled"], json!(false));
            assert!(value["pid"].is_null());
            assert!(value["last_error"].is_null());
            assert!(!registry.is_running(&group));
        }
    }

    #[tokio::test]
    async fn switching_to_mattermost_invalidates_old_start_before_stop_can_run() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let (app, registry) = route_context(&home);
        assert_eq!(route_request(app.clone(), "/api/im/set", Some(json!({
            "group_id":group,"platform":"slack","bot_token":"test-token","app_token":"test-app-token"
        }))).await.0, StatusCode::OK);
        let (generation, _) = registry.begin_start(&group).await;
        let lock = registry.lifecycle_lock(&group);
        let guard = lock.lock().await;
        let request = tokio::spawn(route_request(
            app.clone(),
            "/api/im/set",
            Some(json!({
                "group_id":group,"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"
            })),
        ));
        let store = GroupStore::new(home).expect("store");
        tokio::time::timeout(Duration::from_secs(3), async {
            while cccc_core::im_state::load(&store, &group).expect("state")["config"]["platform"]
                != "mattermost"
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("configuration saved while stop is blocked");
        assert!(!request.is_finished(), "stop still owns no lifecycle lock");
        assert!(!registry.is_generation_current(&group, generation));
        drop(guard);
        assert_eq!(request.await.expect("save result").0, StatusCode::OK);
        assert!(!registry.is_running(&group));
        registry.shutdown().await;
    }

    #[tokio::test]
    async fn superseded_restore_cannot_overwrite_new_platform() {
        for fail_socket in [false, true] {
            let fixture = fixture().await;
            let (_temp, home, group) = scope();
            let (app, registry) = route_context(&home);
            assert_eq!(route_request(app.clone(), "/api/im/set", Some(json!({
                "group_id":group,"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"
            }))).await.0, StatusCode::OK);
            let store = GroupStore::new(home.clone()).expect("store");
            cccc_core::im_state::update(&store, &group, |state| {
                state["enabled"] = json!(true);
                Ok(())
            })
            .expect("enable restore");
            fixture.state.blocked_identity.store(true, Ordering::SeqCst);
            registry.restore_enabled(home.clone(), DaemonClient::new(home.clone()));
            tokio::time::timeout(
                Duration::from_secs(3),
                fixture.state.identity_entered.notified(),
            )
            .await
            .expect("restore reached identity barrier");
            assert_eq!(route_request(app, "/api/im/set", Some(json!({
                "group_id":group,"platform":"slack","bot_token":"test-token","app_token":"test-app-token"
            }))).await.0, StatusCode::OK);
            let expected = cccc_core::im_state::load(&store, &group).expect("new state");
            if fail_socket {
                fixture.state.ws_mode.store(1, Ordering::SeqCst);
            }
            fixture.state.release_identity.notify_one();
            tokio::time::timeout(Duration::from_secs(3), async {
                while registry
                    .restoring
                    .lock()
                    .expect("restoring")
                    .contains(&group)
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("restore completed");
            assert_eq!(
                cccc_core::im_state::load(&store, &group).expect("state"),
                expected
            );
            assert!(!registry.is_running(&group));
            registry.shutdown().await;
        }
    }

    #[tokio::test]
    async fn superseded_http_start_cannot_overwrite_save_stop_unset_or_new_start() {
        for action in [
            "save",
            "same-save",
            "other-platform",
            "stop",
            "unset",
            "start",
        ] {
            for fail_socket in [false, true] {
                let fixture = fixture().await;
                let (_temp, home, group) = scope();
                let app = route_app(&home);
                let config = json!({"group_id":group,"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"});
                assert_eq!(
                    route_request(app.clone(), "/api/im/set", Some(config.clone()))
                        .await
                        .0,
                    StatusCode::OK
                );
                fixture.state.blocked_identity.store(true, Ordering::SeqCst);
                let old = tokio::spawn(route_request(
                    app.clone(),
                    "/api/im/start",
                    Some(json!({"group_id":group})),
                ));
                tokio::time::timeout(
                    Duration::from_secs(3),
                    fixture.state.identity_entered.notified(),
                )
                .await
                .expect("start reached identity barrier");
                let mut replacement = config.clone();
                let path = match action {
                    "save" => {
                        replacement["bot_token"] = json!("changed-test-token");
                        "/api/im/set"
                    }
                    "same-save" => "/api/im/set",
                    "other-platform" => {
                        replacement = json!({"group_id":group,"platform":"slack","bot_token":"test-token","app_token":"test-app-token"});
                        "/api/im/set"
                    }
                    "stop" => "/api/im/stop",
                    "unset" => "/api/im/unset",
                    _ => "/api/im/start",
                };
                assert_eq!(
                    route_request(app.clone(), path, Some(replacement)).await.0,
                    StatusCode::OK
                );
                let store = GroupStore::new(home.clone()).expect("store");
                let expected = cccc_core::im_state::load(&store, &group).expect("new state");
                if fail_socket {
                    fixture.state.ws_mode.store(1, Ordering::SeqCst);
                }
                fixture.state.release_identity.notify_one();
                let result = tokio::time::timeout(Duration::from_secs(3), old)
                    .await
                    .expect("old request completed")
                    .expect("start task");
                assert!(
                    !result.0.is_success(),
                    "{action}, socket failure={fail_socket}"
                );
                assert_eq!(
                    cccc_core::im_state::load(&store, &group).expect("state"),
                    expected,
                    "{action}, socket failure={fail_socket}"
                );
                assert_eq!(
                    route_request(app, "/api/im/stop", Some(json!({"group_id":group})))
                        .await
                        .0,
                    StatusCode::OK
                );
            }
        }
    }

    #[tokio::test]
    async fn failed_final_state_update_removes_only_its_installed_generation() {
        for newer in [false, true] {
            let (_temp, home, group) = scope();
            let (_, registry) = route_context(&home);
            let store = GroupStore::new(home).expect("store");
            let config = json!({"platform":"mattermost"})
                .as_object()
                .expect("config")
                .clone();
            cccc_core::im_state::update(&store, &group, |state| {
                state["config"] = json!(config);
                Ok(())
            })
            .expect("configure");
            let (failed_generation, _) = registry.begin_start(&group).await;
            let generation = if newer {
                registry.begin_start(&group).await.0
            } else {
                failed_generation
            };
            let stopped = Arc::new(AtomicBool::new(false));
            let stop_flag = Arc::clone(&stopped);
            let (closed, disconnected) = tokio::sync::oneshot::channel::<()>();
            registry
                .install(
                    &group,
                    generation,
                    super::super::worker(
                        vec![tokio::spawn(async move {
                            let _closed = closed;
                            std::future::pending::<()>().await;
                        })],
                        Arc::new(move || {
                            stop_flag.store(true, Ordering::SeqCst);
                        }),
                    ),
                )
                .await
                .expect("install worker");
            let path = store
                .group_dir(&group)
                .expect("group dir")
                .join("group.yaml");
            let original = std::fs::read(&path).expect("saved config");
            if !newer {
                // Inject the state-file failure after installation without relying on OS permissions or random timing.
                std::fs::write(&path, "[").expect("corrupt only test state");
            }
            let expected = std::fs::read(&path).expect("expected state");
            assert!(
                complete_start(
                    &registry,
                    &store,
                    &group,
                    &config,
                    failed_generation,
                    Ok(())
                )
                .await
                .is_err()
            );
            assert_eq!(std::fs::read(&path).expect("unchanged state"), expected);
            assert_eq!(registry.is_running(&group), newer);
            assert_eq!(registry.is_generation_current(&group, generation), newer);
            assert_eq!(stopped.load(Ordering::SeqCst), !newer);
            if newer {
                assert!(registry.stop(&group).await);
            }
            tokio::time::timeout(Duration::from_secs(3), disconnected)
                .await
                .expect("worker terminated")
                .expect_err("task sender dropped");
            std::fs::write(&path, original).expect("restore test state");
            let (retry, _) = registry.begin_start(&group).await;
            registry
                .install(
                    &group,
                    retry,
                    super::super::worker(
                        vec![tokio::spawn(std::future::pending())],
                        super::super::no_op_stopper(),
                    ),
                )
                .await
                .expect("retry install");
            complete_start(&registry, &store, &group, &config, retry, Ok(()))
                .await
                .expect("retry commits");
            assert_eq!(
                cccc_core::im_state::load(&store, &group).expect("retry state")["running"],
                true
            );
            registry.shutdown().await;
        }
    }

    #[tokio::test]
    async fn stale_success_and_error_commits_are_both_discarded() {
        let (_temp, home, group) = scope();
        let registry = super::super::ImWorkerRegistry::new(
            crate::ledger_event_hub::LedgerEventHub::new(home.clone()),
        );
        let store = GroupStore::new(home).expect("store");
        let config = json!({"platform":"mattermost"})
            .as_object()
            .expect("config")
            .clone();
        cccc_core::im_state::update(&store, &group, |state| {
            state["config"] = json!(config);
            Ok(())
        })
        .expect("state");
        let (generation, _) = registry.begin_start(&group).await;
        update_start_state(&registry, &store, &group, &config, generation, None).expect("prepare");
        registry.stop(&group).await;
        let expected = cccc_core::im_state::load(&store, &group).expect("state");
        for result in [Ok(()), Err("old failure".to_owned())] {
            assert!(
                update_start_state(
                    &registry,
                    &store,
                    &group,
                    &config,
                    generation,
                    Some(&result)
                )
                .is_err()
            );
            assert_eq!(
                cccc_core::im_state::load(&store, &group).expect("state"),
                expected
            );
        }
    }

    #[tokio::test]
    async fn permanent_reconnect_authentication_failure_stops_registered_worker() {
        for (mode, expected) in [
            (1, "HTTP 401"),
            (6, "HTTP 403"),
            (2, "authentication failed"),
        ] {
            let fixture = fixture().await;
            fixture.state.ws_mode.store(3, Ordering::SeqCst);
            let (_temp, home, group) = scope();
            let app = route_app(&home);
            let config = json!({"group_id":group,"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"});
            assert_eq!(
                route_request(app.clone(), "/api/im/set", Some(config))
                    .await
                    .0,
                StatusCode::OK
            );
            assert_eq!(
                route_request(
                    app.clone(),
                    "/api/im/start",
                    Some(json!({"group_id":group}))
                )
                .await
                .0,
                StatusCode::OK
            );
            fixture.state.ws_mode.store(mode, Ordering::SeqCst);
            tokio::time::timeout(Duration::from_secs(9), async {
                loop {
                    let (_, state) = route_request(
                        app.clone(),
                        &format!("/api/im/status?group_id={group}"),
                        None,
                    )
                    .await;
                    let state = &state["result"];
                    if state["running"] == false
                        && state["last_error"]
                            .as_str()
                            .is_some_and(|error| error.contains(expected))
                    {
                        assert_eq!(state["adapter_available"], false);
                        assert!(state["pid"].is_null());
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("authentication failure stops native worker");
            let attempts = fixture.state.ws_attempts.load(Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5200)).await;
            assert_eq!(
                fixture.state.ws_attempts.load(Ordering::SeqCst),
                attempts,
                "must not retry immutable credentials"
            );
            assert_eq!(attempts, 2);
            assert_eq!(
                route_request(app, "/api/im/stop", Some(json!({"group_id":group})))
                    .await
                    .0,
                StatusCode::OK
            );
        }
    }

    #[tokio::test]
    async fn temporary_websocket_failures_remain_retryable() {
        let fixture = fixture().await;
        for mode in [7, 8, 4] {
            fixture.state.ws_mode.store(mode, Ordering::SeqCst);
            assert!(matches!(
                fixture.api.socket().await,
                Err(SocketError::Retry(_))
            ));
        }
    }

    #[tokio::test]
    async fn disconnected_socket_reconnects_and_clears_persisted_error() {
        let mut fixture = fixture().await;
        fixture.state.ws_mode.store(3, Ordering::SeqCst);
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        fixture.api.worker = Some(worker_state(&home, &group));
        let socket = fixture.api.socket().await.expect("first connection");
        let (inbound, _receiver) = mpsc::channel(128);
        let task = tokio::spawn(socket_loop(
            home,
            group.clone(),
            fixture.api.clone(),
            socket,
            inbound,
            Duration::from_secs(30),
        ));
        let checked = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let state = cccc_core::im_state::load(&store, &group).expect("state");
                if state["last_error"]
                    .as_str()
                    .is_some_and(|s| s.contains("disconnected"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            loop {
                let state = cccc_core::im_state::load(&store, &group).expect("state");
                if fixture.state.ws_connections.load(Ordering::SeqCst) >= 2
                    && state["last_error"].is_null()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        task.abort();
        let _ = task.await;
        checked.expect("log disconnection and clear the error after reconnect");
    }

    #[tokio::test]
    async fn failed_stream_start_and_update_keep_final_message() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let sender = MattermostOutbound::new(home, &group, fixture.api.clone(), &Map::new());
        let target = target(false);
        fixture.state.fail_create.store(true, Ordering::Relaxed);
        assert!(
            sender
                .send_target(&target, &stream(&group, "start", "Initial post failure"))
                .await
                .is_err()
        );
        fixture.state.fail_create.store(false, Ordering::Relaxed);
        let mut final_event = stream(&group, "end", "Complete result");
        final_event.kind = "chat.message".into();
        sender
            .send_target(&target, &final_event)
            .await
            .expect("fallback after initial post failure");
        sender
            .send_target(&target, &stream(&group, "start", "New stream"))
            .await
            .expect("New stream");
        fixture.state.fail_edit.store(true, Ordering::Relaxed);
        assert!(
            sender
                .send_target(&target, &stream(&group, "update", "Update failure"))
                .await
                .is_err()
        );
        sender
            .send_target(&target, &final_event)
            .await
            .expect("fallback after update failure");
        let posts = fixture.state.posts.lock().expect("posts");
        assert_eq!(posts.len(), 3);
        assert_eq!(posts[0]["message"], "**reviewer**\n\nComplete result");
        assert_eq!(posts[2]["message"], posts[0]["message"]);
    }

    #[tokio::test]
    #[ignore = "Requires an authorized test site and channel; leaves three protocol test posts"]
    async fn live_stream_updates_main_and_thread_without_duplicate_final() {
        let site = std::env::var("CCCC_MM_TEST_SITE").expect("test site");
        let channel = std::env::var("CCCC_MM_TEST_CHANNEL").expect("test channel");
        assert!(valid_id(&channel));
        let token_path =
            std::env::var("CCCC_MM_TEST_TOKEN_FILE").expect("test Bot credential file");
        let token = std::fs::read_to_string(token_path).expect("read test Bot credential");
        let config = json!({"mattermost_url":site,"bot_token":token.trim()});
        let api = authenticate(config.as_object().expect("config"))
            .await
            .expect("test Bot identity");
        let (_temp, home, group) = scope();
        let label = format!("Protocol streaming check {group} (not model output)");
        let root = api
            .post(&channel, "", &label, &[])
            .await
            .expect("test root post");
        let targets = [
            AuthorizedChat {
                chat_id: channel.clone(),
                thread_id: String::new(),
                verbose: false,
            },
            AuthorizedChat {
                chat_id: channel.clone(),
                thread_id: root.clone(),
                verbose: false,
            },
        ];
        let sender = MattermostOutbound::new(home, &group, api.clone(), &Map::new());
        let mut post_ids = Vec::new();
        for (op, suffix) in [
            ("start", "Start"),
            ("update", "In progress"),
            ("end", "Done🙂"),
        ] {
            let text = format!("{label}: {suffix}");
            for target in &targets {
                sender
                    .send_target(target, &stream(&group, op, &text))
                    .await
                    .expect("streaming delivery");
            }
            let posts = api
                .json(
                    Method::GET,
                    &format!("channels/{channel}/posts?per_page=20"),
                    None,
                )
                .await
                .expect("read test post");
            for (index, target) in targets.iter().enumerate() {
                let expected = format!("**reviewer**\n\n{text}");
                let matching: Vec<_> = posts["posts"]
                    .as_object()
                    .expect("post list")
                    .values()
                    .filter(|post| {
                        post["message"] == expected && post["root_id"] == target.thread_id
                    })
                    .collect();
                assert_eq!(
                    matching.len(),
                    1,
                    "each target must have one current version"
                );
                let id = field(matching[0], "id").to_owned();
                if op == "start" {
                    post_ids.push(id);
                } else {
                    assert_eq!(id, post_ids[index]);
                }
            }
        }
        let mut final_event = stream(&group, "end", &format!("{label}: Done🙂"));
        final_event.kind = "chat.message".into();
        for target in &targets {
            sender
                .send_target(target, &final_event)
                .await
                .expect("final message deduplication");
        }
        let posts = api
            .json(
                Method::GET,
                &format!("channels/{channel}/posts?per_page=20"),
                None,
            )
            .await
            .expect("read final post");
        let count = posts["posts"]
            .as_object()
            .expect("post list")
            .values()
            .filter(|post| field(post, "message").contains(&label))
            .count();
        assert_eq!(
            count, 3,
            "retain only the root post and two edited target posts"
        );
    }

    #[tokio::test]
    async fn verifies_bot_and_websocket_with_site_subpath() {
        let fixture = fixture().await;
        let config = json!({"mattermost_url":fixture.api.site,"bot_token":"test-token"});
        let api = authenticate(config.as_object().expect("Mattermost test operation"))
            .await
            .expect("Mattermost test operation");
        assert_eq!(api.bot_id, "b".repeat(26));
        let (mut socket, _) = api.socket().await.expect("Mattermost test operation");
        socket.close(None).await.expect("Mattermost test operation");
        let mut invalid = config.clone();
        invalid["bot_token"] = json!("wrong-token");
        let error = authenticate(invalid.as_object().expect("Mattermost test operation"))
            .await
            .err()
            .expect("Mattermost test operation");
        assert!(error.contains("401"));
        assert!(!error.contains("wrong-token"));
    }

    #[tokio::test]
    async fn streams_are_per_target_and_failed_edit_retains_final_fallback() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let sender = MattermostOutbound::new(home, &group, fixture.api.clone(), &Map::new());
        let main = target(false);
        let thread = target(true);
        let first = stream(&group, "start", "");
        sender
            .send_target(&main, &first)
            .await
            .expect("Mattermost test operation");
        sender
            .send_target(&thread, &first)
            .await
            .expect("Mattermost test operation");
        let end = stream(&group, "end", "Result🙂");
        sender
            .send_target(&main, &end)
            .await
            .expect("Mattermost test operation");
        fixture.state.fail_edit.store(true, Ordering::Relaxed);
        assert!(sender.send_target(&thread, &end).await.is_err());
        let mut final_event = end;
        final_event.kind = "chat.message".into();
        sender
            .send_target(&main, &final_event)
            .await
            .expect("Mattermost test operation");
        sender
            .send_target(&thread, &final_event)
            .await
            .expect("Mattermost test operation");
        let posts = fixture
            .state
            .posts
            .lock()
            .expect("Mattermost test operation");
        assert_eq!(posts.len(), 3);
        assert_eq!(posts[0]["root_id"], "");
        assert_eq!(posts[2]["root_id"], "t".repeat(26));
        assert_eq!(posts[2]["message"], "**reviewer**\n\nResult🙂");
    }

    #[tokio::test]
    async fn long_stream_completes_all_posts_once_and_keeps_failed_tail_fallback() {
        for threaded in [false, true] {
            for fail_tail in [false, true] {
                let fixture = fixture().await;
                let (_temp, home, group) = scope();
                let blob =
                    cccc_core::blobs::store(&home, &group, b"final attachment").expect("blob");
                let sender =
                    MattermostOutbound::new(home, &group, fixture.api.clone(), &Map::new());
                let target = target(threaded);
                let raw = "中文🙂".repeat(6000);
                let expected = format!("**reviewer**\n\n{raw}");
                sender
                    .send_target(&target, &stream(&group, "start", ""))
                    .await
                    .expect("start");
                fixture.state.fail_create.store(fail_tail, Ordering::SeqCst);
                assert_eq!(
                    sender
                        .send_target(&target, &stream(&group, "end", &raw))
                        .await
                        .is_err(),
                    fail_tail
                );
                fixture.state.fail_create.store(false, Ordering::SeqCst);
                let mut final_event = stream(&group, "end", &raw);
                final_event.kind = "chat.message".into();
                final_event.data.insert(
                    "attachments".into(),
                    json!([{"path":blob.path,"title":"final.txt"}]),
                );
                sender
                    .send_target(&target, &final_event)
                    .await
                    .expect("final and attachment");
                let posts = fixture.state.posts.lock().expect("all posts");
                let all_text = posts
                    .iter()
                    .map(|post| field(post, "message"))
                    .collect::<String>();
                if fail_tail {
                    // Keep the full fallback after a tail chunk fails; the edited first chunk remains delivered, with no atomic rollback.
                    assert_eq!(
                        all_text,
                        format!("{}{}", field(&posts[0], "message"), expected)
                    );
                } else {
                    assert_eq!(
                        all_text, expected,
                        "include the edited first post without duplicating its chunk"
                    );
                }
                assert!(posts.iter().all(|post| post["root_id"] == target.thread_id
                    && field(post, "message").chars().count() <= 16_383));
                assert_eq!(
                    posts
                        .iter()
                        .filter(|post| post["file_ids"] == json!(["f".repeat(26)]))
                        .count(),
                    1
                );
                assert_eq!(
                    fixture.state.uploads.lock().expect("uploads").as_slice(),
                    &[b"final attachment".to_vec()]
                );
            }
        }
    }

    #[tokio::test]
    async fn final_text_change_and_long_unicode_output_are_not_suppressed() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let sender = MattermostOutbound::new(home, &group, fixture.api.clone(), &Map::new());
        let target = target(false);
        sender
            .send_target(&target, &stream(&group, "start", ""))
            .await
            .expect("Mattermost test operation");
        sender
            .send_target(&target, &stream(&group, "end", "Draft"))
            .await
            .expect("Mattermost test operation");
        let mut final_event = stream(&group, "end", &"中文🙂".repeat(6000));
        final_event.kind = "chat.message".into();
        sender
            .send_target(&target, &final_event)
            .await
            .expect("Mattermost test operation");
        let posts = fixture
            .state
            .posts
            .lock()
            .expect("Mattermost test operation");
        let text = posts[1..]
            .iter()
            .map(|v| field(v, "message"))
            .collect::<String>();
        assert_eq!(text, format!("**reviewer**\n\n{}", "中文🙂".repeat(6000)));
        assert_eq!(
            posts[0]["message"], "**reviewer**\n\nDraft",
            "retain the old draft and the revised final body separately"
        );
        assert!(
            posts
                .iter()
                .all(|p| field(p, "message").chars().count() <= 16_383)
        );
    }

    #[tokio::test]
    async fn attaches_blob_files_to_the_original_thread() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let blob =
            cccc_core::blobs::store(&home, &group, b"document").expect("Mattermost test operation");
        let sender = MattermostOutbound::new(home, &group, fixture.api.clone(), &Map::new());
        let mut event = Event::new("chat.message", &group);
        event.by = "reviewer".into();
        event.data.insert(
            "attachments".into(),
            json!([{"path":blob.path,"title":"报告.txt"}]),
        );
        sender
            .send_target(&target(true), &event)
            .await
            .expect("Mattermost test operation");
        assert_eq!(
            *fixture
                .state
                .uploads
                .lock()
                .expect("Mattermost test operation"),
            vec![b"document".to_vec()]
        );
        let posts = fixture
            .state
            .posts
            .lock()
            .expect("Mattermost test operation");
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0]["file_ids"], json!(["f".repeat(26)]));
        assert_eq!(posts[0]["root_id"], "t".repeat(26));
        assert_eq!(
            *fixture
                .state
                .upload_metadata
                .lock()
                .expect("upload metadata"),
            vec![
                json!({"channel_id":"c".repeat(26),"filename":"报告.txt","content_type":"application/octet-stream"})
            ]
        );
    }

    #[tokio::test]
    async fn upload_preserves_query_metadata_and_special_filename() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let blob = cccc_core::blobs::store(&home, &group, b"raw document").expect("Blob");
        let sender = MattermostOutbound::new(home, &group, fixture.api.clone(), &Map::new());
        let mut event = Event::new("chat.message", &group);
        event.by = "reviewer".into();
        let title = "报告 #1 + &.txt";
        event.data.insert(
            "attachments".into(),
            json!([{"path":blob.path,"title":title}]),
        );
        sender
            .send_target(&target(true), &event)
            .await
            .expect("upload and post");
        assert_eq!(
            *fixture
                .state
                .upload_metadata
                .lock()
                .expect("upload metadata"),
            vec![
                json!({"channel_id":"c".repeat(26),"filename":title,"content_type":"application/octet-stream"})
            ]
        );
        assert_eq!(
            *fixture.state.uploads.lock().expect("uploaded content"),
            vec![b"raw document".to_vec()]
        );
        let posts = fixture.state.posts.lock().expect("posts");
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0]["channel_id"], "c".repeat(26));
        assert_eq!(posts[0]["root_id"], "t".repeat(26));
        assert_eq!(posts[0]["file_ids"], json!(["f".repeat(26)]));
    }

    #[tokio::test]
    async fn unauthorized_attachments_are_not_downloaded_and_pairing_preserves_thread() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
        let mut inbound = MattermostInbound::new(
            home.clone(),
            &group,
            DaemonClient::new(home.clone()),
            fixture.api.clone(),
            reactions,
            &Map::new(),
        );
        let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"t".repeat(26),"message":"@cccc_bot See attachments","type":"","file_ids":["f".repeat(26)]});
        let event = json!({"event":"posted","data":{"channel_type":"O","post":post.to_string()}});
        inbound.handle(&event).await.expect("unauthorized request");
        assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
        let mut subscribe = post.clone();
        subscribe["id"] = json!("q".repeat(26));
        subscribe["message"] = json!("@cccc_bot /subscribe");
        let event =
            json!({"event":"posted","data":{"channel_type":"O","post":subscribe.to_string()}});
        inbound.handle(&event).await.expect("pairing request");
        inbound
            .handle(&event)
            .await
            .expect("duplicate pairing request");
        let store = GroupStore::new(home).expect("store");
        let state = cccc_core::im_state::load(&store, &group).expect("state");
        assert_eq!(state["pending"].as_array().expect("pending").len(), 1);
        assert_eq!(state["pending"][0]["thread_id"], "t".repeat(26));
        let posts = fixture.state.posts.lock().expect("posts");
        assert_eq!(posts.len(), 2);
        assert!(posts.iter().all(|p| p["root_id"] == "t".repeat(26)));
    }

    #[tokio::test]
    async fn rejects_redirects_and_retries_only_explicit_rate_rejection() {
        let fixture = fixture().await;
        let error = fixture
            .api
            .json(Method::GET, "test-redirect", None)
            .await
            .expect_err("redirect rejected");
        assert!(error.contains("307"));
        let started = Instant::now();
        let response = fixture
            .api
            .json(Method::GET, "test-rate", None)
            .await
            .expect("rate retry");
        assert_eq!(response["ok"], true);
        assert!(started.elapsed() >= Duration::from_secs(1));
        assert_eq!(*fixture.state.rate_requests.lock().expect("requests"), 2);
    }

    #[tokio::test]
    async fn reactions_match_the_source_event_and_only_remove_own_reactions() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let reactions = MattermostReactions::new(home, &group, fixture.api.clone());
        let key = target(true).key();
        let first = "p".repeat(26);
        let second = "q".repeat(26);
        reactions.start(&key, &first).await;
        reactions.bind(&key, &first, "event-1".into());
        reactions.start(&key, &second).await;
        reactions.bind(&key, &second, "event-2".into());
        reactions.complete(&key, Some("unrelated"), true).await;
        assert_eq!(reactions.active.len(&key), 2);
        reactions.complete(&key, Some("event-2"), true).await;
        assert_eq!(reactions.active.len(&key), 1);
        let calls = fixture.state.reactions.lock().expect("calls");
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[2].0, Method::DELETE);
        assert_eq!(
            calls[2].1,
            format!(
                "/sub/api/v4/users/{}/posts/{second}/reactions/eyes",
                fixture.api.bot_id
            )
        );
        assert_eq!(calls[3].2["post_id"], second);
        assert_eq!(calls[3].2["emoji_name"], "white_check_mark");
    }

    #[tokio::test]
    async fn completion_waits_for_dispatch_binding_and_is_applied_only_once() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let reactions = MattermostReactions::new(home, &group, fixture.api.clone());
        let key = target(true).key();
        for success in [true, false] {
            let post = if success { "p" } else { "q" }.repeat(26);
            reactions.start(&key, &post).await;
            let binding = reactions.binding.lock().await;
            let completion = reactions.complete(&key, Some("early-event"), success);
            tokio::pin!(completion);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), &mut completion)
                    .await
                    .is_err()
            );
            assert_eq!(reactions.active.len(&key), 1);
            reactions.bind(&key, &post, "early-event".into());
            drop(binding);
            completion.await;
            reactions.complete(&key, Some("early-event"), success).await;
            assert_eq!(reactions.active.len(&key), 0);
            let calls = fixture.state.reactions.lock().expect("calls");
            assert_eq!(
                calls.last().expect("completion").2["emoji_name"],
                if success { "white_check_mark" } else { "x" }
            );
        }
        assert_eq!(fixture.state.reactions.lock().expect("calls").len(), 6);
        // After failure or cancellation releases the lock, unrelated completions must not clear other requests' reactions.
        let post = "r".repeat(26);
        reactions.start(&key, &post).await;
        let binding = reactions.binding.lock().await;
        drop(binding);
        reactions.complete(&key, Some("unrelated"), true).await;
        assert_eq!(reactions.active.len(&key), 1);
        reactions.fail_post(&key, &post).await;
        assert_eq!(reactions.active.len(&key), 0);
    }

    #[tokio::test]
    async fn socket_backpressure_keeps_sending_heartbeats_and_preserves_order() {
        let fixture = fixture().await;
        fixture.state.ws_mode.store(5, Ordering::SeqCst);
        *fixture.state.ws_events.lock().expect("events") =
            (0..3).map(|id| json!({"event":"posted","id":id})).collect();
        let (_temp, home, group) = scope();
        let socket = fixture.api.socket().await.expect("socket");
        let (sender, mut receiver) = mpsc::channel(1);
        let task = tokio::spawn(socket_loop(
            home,
            group,
            fixture.api.clone(),
            socket,
            sender,
            Duration::from_millis(20),
        ));
        // Leave the queue unconsumed for over three heartbeat periods; local backpressure must not count as a remote timeout.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(receiver.len(), 1);
        assert!(fixture.state.ws_pings.load(Ordering::SeqCst) >= 3);
        assert_eq!(fixture.state.ws_connections.load(Ordering::SeqCst), 1);
        for id in 0..3 {
            let event = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
                .await
                .expect("event timeout")
                .expect("event");
            assert_eq!(event["id"], id);
        }
        drop(receiver);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("worker shutdown")
            .expect("socket task");
    }

    #[tokio::test]
    async fn slow_attachment_does_not_block_socket_pongs_or_reorder_inbound() {
        let fixture = fixture().await;
        fixture.state.ws_mode.store(5, Ordering::SeqCst);
        fixture.state.blocked_download.store(true, Ordering::SeqCst);
        let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot See attachments","type":"","file_ids":["f".repeat(26)]});
        let mut help = post.clone();
        help["id"] = json!("q".repeat(26));
        help["message"] = json!("@cccc_bot /help");
        help["file_ids"] = json!([]);
        *fixture.state.ws_events.lock().expect("events") = [post, help].into_iter()
            .map(|post| json!({"event":"posted","data":{"channel_type":"O","post":post.to_string()}})).collect();
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        let config = json!({"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"});
        let config = config.as_object().expect("config");
        cccc_core::im_state::update(&store, &group, |state| {
            state["config"] = json!(config);
            Ok(())
        })
        .expect("config");
        verify_identity(&home, &group, &fixture.api, config).expect("identity");
        cccc_core::im_state::update(&store, &group, |state| {
            state["authorized"] = json!([{"platform":"mattermost","chat_id":"c".repeat(26),"thread_id":0,"authorized_at":1}]);
            Ok(())
        }).expect("authorize");
        let tasks = start(
            home.clone(),
            DaemonClient::new(home.clone()),
            &group,
            config,
            crate::ledger_event_hub::LedgerEventHub::new(home.clone()),
            worker_state(&home, &group),
        )
        .await
        .expect("start");
        assert_eq!(tasks.len(), 4);
        let checked = tokio::time::timeout(Duration::from_secs(5), async {
            while *fixture.state.downloads.lock().expect("downloads") == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            while fixture.state.ws_pongs.load(Ordering::SeqCst) < 3 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(fixture.state.posts.lock().expect("posts").is_empty());
            fixture.state.release_download.notify_one();
            while fixture.state.posts.lock().expect("posts").len() < 2 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let posts = fixture.state.posts.lock().expect("posts");
            assert!(
                field(&posts[0], "message")
                    .contains("Could not deliver the message or attachments to CCCC")
            );
            assert!(field(&posts[1], "message").contains("/help"));
        })
        .await;
        for task in tasks {
            task.abort();
            let _ = task.await;
        }
        checked.expect("slow attachment and following help");
    }

    #[tokio::test]
    async fn identity_change_clears_authorization_but_token_rotation_does_not() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("Mattermost test operation");
        let config = json!({"platform":"mattermost","mattermost_url":fixture.api.site,"bot_token":"test-token"});
        cccc_core::im_state::update(&store, &group, |s| {
            s["config"] = config.clone();
            Ok(())
        })
        .expect("Mattermost test operation");
        verify_identity(
            &home,
            &group,
            &fixture.api,
            config.as_object().expect("Mattermost test operation"),
        )
        .expect("Mattermost test operation");
        let authorized = json!([{"platform":"mattermost","chat_id":"c".repeat(26),"thread_id":0,"authorized_at":1}]);
        cccc_core::im_state::update(&store, &group, |s| {
            s["authorized"] = authorized.clone();
            Ok(())
        })
        .expect("Mattermost test operation");
        let mut rotated = config.clone();
        rotated["bot_token"] = json!("rotated-token");
        cccc_core::im_state::update(&store, &group, |s| {
            s["config"] = rotated.clone();
            Ok(())
        })
        .expect("Mattermost test operation");
        let mut same_bot = fixture.api.clone();
        same_bot.token = "rotated-token".into();
        verify_identity(
            &home,
            &group,
            &same_bot,
            rotated.as_object().expect("Mattermost test operation"),
        )
        .expect("Mattermost test operation");
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("Mattermost test operation")["authorized"]
                .as_array()
                .expect("Mattermost test operation")
                .len(),
            1
        );
        same_bot.bot_id = "n".repeat(26);
        verify_identity(
            &home,
            &group,
            &same_bot,
            rotated.as_object().expect("Mattermost test operation"),
        )
        .expect("Mattermost test operation");
        assert!(
            cccc_core::im_state::load(&store, &group).expect("Mattermost test operation")["authorized"]
                .as_array()
                .expect("Mattermost test operation")
                .is_empty()
        );
        assert!(
            verify_identity(
                &home,
                &group,
                &same_bot,
                config.as_object().expect("Mattermost test operation")
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn stale_runtime_errors_and_identity_cannot_mutate_current_state() {
        let mut fixture = fixture().await;
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        let worker = worker_state(&home, &group);
        fixture.api.worker = Some(worker.clone());
        fixture
            .api
            .persist_error(&home, &group, Some("test-token failure"));
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("state")["last_error"],
            "[REDACTED] failure"
        );
        // A new startup with the same config must reject stale writes, including error clearing.
        worker
            .generations
            .lock()
            .expect("generations")
            .insert(group.clone(), 2);
        let expected = cccc_core::im_state::load(&store, &group).expect("state");
        for error in [Some("old error"), None] {
            fixture.api.persist_error(&home, &group, error);
        }
        assert!(verify_identity(&home, &group, &fixture.api, &worker.config).is_err());
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("state"),
            expected
        );
        assert!(
            !store
                .state_dir(&group)
                .expect("dir")
                .join("mattermost_identity.json")
                .exists()
        );
        // Config changes must independently reject stale state writes without requiring the worker to stop first.
        worker
            .generations
            .lock()
            .expect("generations")
            .insert(group.clone(), 1);
        cccc_core::im_state::update(&store, &group, |s| {
            s["config"] = json!({"platform":"telegram"});
            Ok(())
        })
        .expect("save");
        let expected = cccc_core::im_state::load(&store, &group).expect("state");
        for error in [Some("old error"), None] {
            fixture.api.persist_error(&home, &group, error);
        }
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("state"),
            expected
        );
    }

    #[tokio::test]
    async fn identity_commit_is_serialized_and_failure_keeps_authorization_cleared() {
        let fixture = fixture().await;
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        let config = worker_state(&home, &group).config;
        let authorized = json!([{"platform":"mattermost","chat_id":"c".repeat(26),"thread_id":0,"authorized_at":1}]);
        cccc_core::im_state::update(&store, &group, |s| {
            s["authorized"] = authorized.clone();
            Ok(())
        })
        .expect("authorize");
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let old_home = home.clone();
        let old_group = group.clone();
        let old_api = fixture.api.clone();
        let old_config = config.clone();
        let old = std::thread::spawn(move || {
            verify_identity_with(
                &old_home,
                &old_group,
                &old_api,
                &old_config,
                |path, identity| {
                    let store = GroupStore::new(old_home.clone())?;
                    assert_eq!(
                        cccc_core::im_state::load(&store, &old_group)?["authorized"],
                        json!([])
                    );
                    entered_tx.send(()).expect("entered");
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release old commit");
                    cccc_core::fs::write_json_committed(path, identity)
                },
            )
        });
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("old commit reached");
        let mut new_api = fixture.api.clone();
        new_api.bot_id = "n".repeat(26);
        let new_home = home.clone();
        let new_group = group.clone();
        let new_config = config.clone();
        let new_api_task = new_api.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let new = std::thread::spawn(move || {
            started_tx.send(()).expect("started");
            let result = verify_identity(&new_home, &new_group, &new_api_task, &new_config);
            finished_tx.send(()).expect("finished");
            result
        });
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("new started");
        let blocked = finished_rx
            .recv_timeout(Duration::from_millis(150))
            .is_err();
        release_tx.send(()).expect("release");
        old.join().expect("old thread").expect("old commit");
        new.join().expect("new thread").expect("new commit");
        assert!(
            blocked,
            "new identity must not overtake an old in-flight commit"
        );
        let path = store
            .state_dir(&group)
            .expect("dir")
            .join("mattermost_identity.json");
        let identity: Value = cccc_core::fs::read_json(&path).expect("identity");
        assert_eq!(identity["bot_id"], new_api.bot_id);
        cccc_core::im_state::update(&store, &group, |s| {
            s["authorized"] = authorized.clone();
            Ok(())
        })
        .expect("authorize new");
        verify_identity(&home, &group, &new_api, &config).expect("restart new");
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("state")["authorized"],
            authorized
        );
        assert!(
            verify_identity_with(&home, &group, &fixture.api, &config, |_, _| Err(
                std::io::Error::other("injected identity write failure")
            ))
            .is_err()
        );
        assert_eq!(
            cccc_core::im_state::load(&store, &group).expect("state")["authorized"],
            json!([])
        );
        assert_eq!(
            cccc_core::fs::read_json::<Value>(&path).expect("identity"),
            identity
        );
    }

    #[test]
    fn socket_cursor_handles_replies_duplicates_gaps_and_new_connections() {
        let mut cursor = SocketCursor::default();
        assert!(
            cursor
                .accept(&json!({"event":"hello","data":{"connection_id":"s".repeat(26)},"seq":0}))
                .expect("hello")
        );
        assert!(
            !cursor
                .accept(&json!({"seq_reply":1,"status":"OK"}))
                .expect("reply")
        );
        assert!(
            cursor
                .accept(&json!({"event":"status_change","seq":1}))
                .expect("status")
        );
        assert!(
            !cursor
                .accept(&json!({"event":"posted","seq":1}))
                .expect("duplicate")
        );
        assert!(cursor.accept(&json!({"event":"posted","seq":3})).is_err());
        assert_eq!(cursor.next_sequence, 2);
        assert!(
            cursor
                .accept(&json!({"event":"posted","seq":2}))
                .expect("recovered")
        );
        assert!(
            cursor
                .accept(&json!({"event":"hello","data":{"connection_id":"n".repeat(26)},"seq":0}))
                .expect("new hello")
        );
        assert!(cursor.missed_events);
        assert_eq!(cursor.next_sequence, 1);
        assert!(cursor.accept(&json!({"event":"posted"})).is_err());
        assert_eq!(cursor.next_sequence, 1);
        assert!(!SocketCursor::default().missed_events);
    }

    #[tokio::test]
    #[ignore = "Requires an authorized test site and channel; leaves one Bot-authored post and makes no model calls"]
    async fn live_native_websocket_recovers_post_sent_while_disconnected() {
        let site = std::env::var("CCCC_MM_TEST_SITE").expect("test site");
        let channel = std::env::var("CCCC_MM_TEST_CHANNEL").expect("test channel");
        let token = std::fs::read_to_string(
            std::env::var("CCCC_MM_TEST_TOKEN_FILE").expect("credential file"),
        )
        .expect("read credential");
        let config = json!({"mattermost_url":site,"bot_token":token.trim()});
        let api = authenticate(config.as_object().expect("config"))
            .await
            .expect("Bot identity");
        let (socket, mut cursor) = api.socket().await.expect("initial connection");
        let connection_id = cursor.connection_id.clone();
        drop(socket);
        let post_id = api
            .post(
                &channel,
                "",
                "CCCC connector protocol test: recover a post sent during disconnection. This post is sent by a test Bot with no model calls.",
                &[],
            )
            .await
            .expect("post during disconnection");
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut recovered = api.open_socket(&cursor).await.expect("resume connection");
        tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(message) = recovered.next().await {
                match message.expect("event frame") {
                    Message::Text(text) => {
                        let event: Value = serde_json::from_str(&text).expect("JSON");
                        if cursor.accept(&event).expect("consecutive sequence")
                            && field(&event, "event") == "posted"
                        {
                            let post: Value =
                                serde_json::from_str(field(&event["data"], "post")).expect("posts");
                            if field(&post, "id") == post_id {
                                return;
                            }
                        }
                    }
                    Message::Ping(data) => recovered.send(Message::Pong(data)).await.expect("pong"),
                    _ => {}
                }
            }
            panic!("connection closed before replay");
        })
        .await
        .expect("replay test post from the actual server");
        assert_eq!(cursor.connection_id, connection_id);
        assert!(!cursor.missed_events);
        recovered.close(None).await.expect("close test connection");
    }

    #[tokio::test]
    async fn websocket_recovers_without_hello_and_preserves_order_and_current_authorization() {
        let mut fixture = fixture().await;
        fixture.state.ws_mode.store(9, Ordering::SeqCst);
        let (_temp, home, group) = scope();
        fixture.api.worker = Some(worker_state(&home, &group));
        let socket = fixture.api.socket().await.expect("initial hello");
        let (sender, mut receiver) = mpsc::channel(1);
        let task = tokio::spawn(socket_loop(
            home.clone(),
            group.clone(),
            fixture.api.clone(),
            socket,
            sender,
            Duration::from_secs(30),
        ));
        let checked = tokio::time::timeout(Duration::from_secs(12), async {
            let mut events = Vec::new();
            for expected in [1, 2, 4] {
                let event = receiver.recv().await.expect("recovered event");
                assert_eq!(event["seq"], expected);
                events.push(event);
            }
            let queries = fixture.state.ws_queries.lock().expect("queries").clone();
            assert_eq!(queries.len(), 2);
            assert_eq!(queries[0]["connection_id"], "");
            assert_eq!(queries[1]["connection_id"], "s".repeat(26));
            assert_eq!(queries[1]["sequence_number"], "2");
            assert!(
                receiver.try_recv().is_err(),
                "duplicates and non-posted events must not enter inbound"
            );
            let store = GroupStore::new(home.clone()).expect("store");
            assert!(
                cccc_core::im_state::load(&store, &group).expect("state")["last_error"].is_null()
            );
            let reactions = MattermostReactions::new(home.clone(), &group, fixture.api.clone());
            let mut inbound = MattermostInbound::new(
                home.clone(),
                &group,
                DaemonClient::new(home.clone()),
                fixture.api.clone(),
                reactions,
                &Map::new(),
            );
            let mut recovered = events[1].clone();
            let mut post: Value =
                serde_json::from_str(field(&recovered["data"], "post")).expect("post");
            post["message"] = json!("@cccc_bot Unauthorized replayed attachment");
            post["file_ids"] = json!(["f".repeat(26)]);
            recovered["data"]["post"] = json!(post.to_string());
            inbound
                .handle(&recovered)
                .await
                .expect("current authorization");
            assert_eq!(*fixture.state.downloads.lock().expect("downloads"), 0);
            assert!(
                field(&fixture.state.posts.lock().expect("posts")[0], "message")
                    .contains("not authorized")
            );
        })
        .await;
        task.abort();
        let _ = task.await;
        checked.expect("native recovery");
    }

    #[tokio::test]
    async fn lost_recovery_cache_remains_visible_after_later_reconnect() {
        let mut fixture = fixture().await;
        fixture.state.ws_mode.store(10, Ordering::SeqCst);
        let (_temp, home, group) = scope();
        fixture.api.worker = Some(worker_state(&home, &group));
        let store = GroupStore::new(home.clone()).expect("store");
        let socket = fixture.api.socket().await.expect("initial");
        let (sender, _receiver) = mpsc::channel(1);
        let task = tokio::spawn(socket_loop(
            home,
            group.clone(),
            fixture.api.clone(),
            socket,
            sender,
            Duration::from_secs(30),
        ));
        let checked = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if fixture.state.ws_connections.load(Ordering::SeqCst) >= 3
                    && cccc_core::im_state::load(&store, &group).expect("state")["last_error"]
                        == RECOVERY_GAP
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let log = std::fs::read_to_string(
                store.state_dir(&group).expect("dir").join("im_bridge.log"),
            )
            .expect("log");
            assert!(log.contains(RECOVERY_GAP));
            assert!(!log.contains("test-token"));
        })
        .await;
        task.abort();
        let _ = task.await;
        checked.expect("loss reported and retained");
    }
}
