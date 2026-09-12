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

#[derive(Clone)]
pub(super) struct MattermostApi {
    pub http: reqwest::Client,
    site: String,
    token: String,
    pub bot_id: String,
    pub username: String,
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

    pub(super) async fn authenticate(config: &Map<String, Value>) -> Result<Self, String> {
        let site = config
            .get("mattermost_url")
            .and_then(Value::as_str)
            .and_then(cccc_core::im_state::normalize_mattermost_url)
            .ok_or("Mattermost site URL is invalid")?;
        let token = resolve_config_credential(config, "bot_token", "bot_token_env")?;
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
        // 仅重试服务器明确拒绝的限流请求；连接中断时不盲目重复创建帖子。
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

    async fn socket(&self) -> Result<Socket, String> {
        // HTTP 升级复用 reqwest 的 TLS、HTTP_PROXY/HTTPS_PROXY/NO_PROXY，无额外代理服务。
        let key = generate_key();
        let response = self
            .request(Method::GET, "websocket")
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
            return Err(format!(
                "Mattermost WebSocket upgrade failed (HTTP {})",
                response.status().as_u16()
            ));
        }
        let upgraded = response.upgrade().await.map_err(http_error)?;
        let mut socket = WebSocketStream::from_raw_socket(upgraded, Role::Client, None).await;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                match socket.next().await {
                    Some(Ok(Message::Text(text))) => {
                        let event: Value = serde_json::from_str(&text)
                            .map_err(|_| "Invalid Mattermost WebSocket JSON")?;
                        if field(&event, "event") == "hello" {
                            return Ok(());
                        }
                        if event.get("error").is_some_and(|v| !v.is_null()) {
                            return Err("Mattermost WebSocket authentication failed");
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        socket
                            .send(Message::Pong(data))
                            .await
                            .map_err(|_| "Mattermost WebSocket ping failed")?;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        return Err("Mattermost WebSocket closed before hello");
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| "Mattermost WebSocket hello timed out")??;
        Ok(socket)
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

pub(super) async fn start(
    home: HomeLayout,
    daemon: DaemonClient,
    group_id: &str,
    config: &Map<String, Value>,
    ledger_events: crate::ledger_event_hub::LedgerEventHub,
) -> Result<Vec<JoinHandle<()>>, String> {
    let token = resolve_config_credential(config, "bot_token", "bot_token_env").unwrap_or_default();
    let api = MattermostApi::authenticate(config)
        .await
        .inspect_err(|error| {
            log_error(&home, group_id, "authenticate", error, &token);
        })?;
    let socket = api.socket().await.inspect_err(|error| {
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
    // 沿用 WeCom 的有界队列与独立入站任务，附件 REST 不占用心跳循环。
    let (inbound_tx, mut inbound_rx) = mpsc::channel(128);
    let inbound_home = home.clone();
    let inbound_group = group_id.to_owned();
    let inbound_api = api.clone();
    let inbound = tokio::spawn(async move {
        while let Some(event) = inbound_rx.recv().await {
            if let Err(error) = inbound.handle(&event).await {
                inbound_api.log_error(&inbound_home, &inbound_group, "inbound", &error);
                persist_error(&inbound_home, &inbound_group, Some(&error));
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
            async move {
                for target in targets {
                    let result = sender.send_target(&target, &event).await;
                    if let Err(error) = &result {
                        log_error(&home, &event.group_id, "send", error, &token);
                        persist_error(&home, &event.group_id, Some(error));
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
    let store = GroupStore::new(home.clone()).map_err(|e| e.to_string())?;
    let path = store
        .state_dir(group_id)
        .map_err(|e| e.to_string())?
        .join("mattermost_identity.json");
    let expected = json!({"site":api.site,"bot_id":api.bot_id});
    let previous: Value = match cccc_core::fs::read_json(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) => return Err(format!("Cannot read Mattermost identity: {error}")),
    };
    cccc_core::im_state::update(&store, group_id, |state| {
        // 配置已被另一个保存请求替换时，本次启动不能修改新配置的授权。
        if state.get("config").and_then(Value::as_object) != Some(config) {
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
    })
    .map_err(|e| e.to_string())?;
    // 先持久清除旧授权，再记录身份；中途失败重试时只会再次清除，不会错误复用。
    cccc_core::fs::write_json_committed(&path, &expected).map_err(|e| e.to_string())
}

// CLI 的组合 Web 入口没有 tracing subscriber；组日志必须独立于该全局初始化。
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

pub(super) fn persist_error(home: &HomeLayout, group_id: &str, error: Option<&str>) {
    let result = GroupStore::new(home.clone()).and_then(|store| {
        cccc_core::im_state::update(&store, group_id, |state| {
            state["last_error"] = error.map_or(Value::Null, |v| json!(v));
            Ok(())
        })
    });
    if let Err(error) = result {
        eprintln!("Mattermost error state write failed ({:?})", error.kind());
    }
}

async fn socket_loop(
    home: HomeLayout,
    group_id: String,
    api: MattermostApi,
    mut socket: Socket,
    inbound: mpsc::Sender<Value>,
    heartbeat_interval: Duration,
) {
    loop {
        let mut heartbeat = tokio::time::interval(heartbeat_interval);
        let mut last_received = tokio::time::Instant::now();
        let mut permit = None;
        let error = loop {
            tokio::select! {
                capacity = inbound.reserve(), if permit.is_none() => {
                    let Ok(capacity) = capacity else { return; };
                    permit = Some(capacity);
                    // 本地背压不是远端失活；恢复读取后才重新计算接收超时。
                    last_received = tokio::time::Instant::now();
                }
                message = socket.next(), if permit.is_some() => {
                    last_received = tokio::time::Instant::now();
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<Value>(&text) {
                                Ok(event) => if field(&event, "event") == "posted" {
                                    if let Some(capacity) = permit.take() { capacity.send(event); }
                                },
                                Err(_) => log_error(&home, &group_id, "decode", "invalid Mattermost event JSON", &api.token),
                            }
                        }
                        Some(Ok(Message::Ping(data))) => { if socket.send(Message::Pong(data)).await.is_err() { break "Mattermost pong failed"; } }
                        Some(Ok(Message::Close(_))) | None => break "Mattermost WebSocket disconnected",
                        Some(Err(_)) => break "Mattermost WebSocket read failed",
                        _ => {}
                    }
                }
                _ = heartbeat.tick() => {
                    if permit.is_some() && last_received.elapsed() > heartbeat_interval * 3 { break "Mattermost WebSocket heartbeat timed out"; }
                    if socket.send(Message::Ping(Vec::new().into())).await.is_err() { break "Mattermost WebSocket ping failed"; }
                }
            }
        };
        persist_error(&home, &group_id, Some(error));
        log_error(&home, &group_id, "disconnect", error, &api.token);
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            match api.socket().await {
                Ok(next) => {
                    socket = next;
                    persist_error(&home, &group_id, None);
                    break;
                }
                Err(error) => {
                    persist_error(&home, &group_id, Some(&error));
                    log_error(&home, &group_id, "reconnect", &error, &api.token);
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
    fn new(home: HomeLayout, group_id: &str, api: MattermostApi) -> Self {
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
        self.finish(self.active.take_where(key, |r| r.post_id == post_id), false)
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
        self.finish(reaction, success).await;
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
    async fn finish(&self, reaction: Option<MattermostReaction>, success: bool) {
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
                    reactions.finish(Some(reaction), false).await;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::AuthorizedChat;
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
        ws_events: Mutex<Vec<Value>>,
        ws_pings: AtomicUsize,
        ws_pongs: AtomicUsize,
        blocked_download: AtomicBool,
        release_download: tokio::sync::Notify,
        uploads: Mutex<Vec<Vec<u8>>>,
        downloads: Mutex<usize>,
        rate_requests: Mutex<usize>,
        reactions: Mutex<Vec<(Method, String, Value)>>,
        forbidden: AtomicBool,
        other_user_is_bot: AtomicBool,
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

    async fn fixture_for_bot(token: &str, bot_id: &str) -> Fixture {
        async fn ws(
            State(state): State<Arc<MockState>>,
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
            if mode == 1 {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            let connection = state.ws_connections.fetch_add(1, Ordering::SeqCst);
            upgrade.on_upgrade(move |mut socket| async move {
                let text = match mode {
                    2 => json!({"error":{"message":"测试认证拒绝"}}).to_string(),
                    4 => "invalid-json".to_owned(),
                    _ => json!({"event":"hello"}).to_string(),
                };
                socket
                    .send(axum::extract::ws::Message::Text(text.into()))
                    .await
                    .expect("Mattermost test operation");
                if mode == 3 && connection == 0 {
                    let _ = socket.send(axum::extract::ws::Message::Close(None)).await;
                    return;
                }
                let events = state.ws_events.lock().expect("events").clone();
                for event in events {
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
                return axum::Json(json!({"id":state.bot_id,"username":"cccc_bot","is_bot":true}))
                    .into_response();
            }
            if path.starts_with("/sub/api/v4/users/") && method == Method::GET {
                return axum::Json(json!({"id":path.rsplit('/').next(),"is_bot":state.other_user_is_bot.load(Ordering::SeqCst)}))
                    .into_response();
            }
            if path.starts_with("/sub/api/v4/files/") && method == Method::GET {
                *state.downloads.lock().expect("downloads") += 1;
                if state.blocked_download.load(Ordering::SeqCst) {
                    state.release_download.notified().await;
                }
                return StatusCode::NOT_FOUND.into_response();
            }
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

    #[test]
    fn group_error_log_appends_redacts_and_isolates_without_tracing() {
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        let other = store.create("Other", "").expect("other group");
        let error = "附件错误\nBearer synthetic-secret";
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
        assert_eq!(rows[0]["error"], "附件错误\nBearer [REDACTED]");
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
        persist_error(&home, &group, None);
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
        log_error(&home, &group, "inbound", "test", ""); // 记录失败不能导致收发任务 panic。
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
        // 故意向明文测试端口发 TLS，不能靠关闭证书验证使其通过。
        let config = json!({"mattermost_url":fixture.api.site.replacen("http://", "https://", 1),"bot_token":"tls-test-secret"});
        let result = MattermostApi::authenticate(config.as_object().expect("config")).await;
        let error = match result {
            Ok(_) => panic!("TLS 协议错误不应认证成功"),
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
            let result = MattermostApi::authenticate(config.as_object().expect("config")).await;
            if case == "rejected" {
                let error = match result {
                    Ok(_) => panic!("失效代理不应认证成功"),
                    Err(error) => error,
                };
                assert!(!error.is_empty());
                assert!(!error.contains("test-token"));
                return;
            }
            let api = result.unwrap_or_else(|error| panic!("REST: {error}"));
            let socket = api.socket().await;
            if case == "ws_rejected" {
                let error = socket.expect_err("代理后的 WS 拒绝应明确返回");
                assert!(error.contains("401"));
                assert!(!error.contains("test-token"));
            } else {
                socket.expect("REST 和 WS 均应使用同一代理策略");
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
            // .invalid 目标只能经本地代理抵达模拟服务，避免直接连接掩盖代理未生效。
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
                    .expect("子进程超时")
                    .expect("子进程执行");
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
        event
            .data
            .insert("text".into(), json!("反应失败不应吞掉正文"));
        event.data.insert(
            "attachments".into(),
            json!([{"path":blob.path,"title":"test.txt"}]),
        );
        assert!(
            sender
                .send_target(&target, &event)
                .await
                .expect_err("附件失败")
                .contains("attachments")
        );
        reactions.complete(&key, Some("event"), false).await;
        assert_eq!(reactions.active.len(&key), 0);
        let posts = fixture.state.posts.lock().expect("posts");
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0]["message"], "**reviewer**\n\n反应失败不应吞掉正文");
        assert!(field(&posts[1], "message").contains("部分附件发送失败"));
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
        post["message"] = json!("普通聊天，无需机器人回答");
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
        event.data.insert("text".into(), json!("只有本组正文"));
        event.data.insert(
            "attachments".into(),
            json!([{"path":blob.path,"title":"test.txt"}]),
        );
        assert!(sender.send_target(&target(false), &event).await.is_err());
        assert!(fixture.state.uploads.lock().expect("uploads").is_empty());
        assert_eq!(
            fixture.state.posts.lock().expect("posts")[0]["message"],
            "**reviewer**\n\n只有本组正文"
        );
        fixture.state.fail_create.store(true, Ordering::SeqCst);
        assert!(
            fixture
                .api
                .post(&"c".repeat(26), "", "禁止重复创建", &[])
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
            let api = MattermostApi::authenticate(saved["config"].as_object().expect("config"))
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
        let result = MattermostApi::authenticate(wrong.as_object().expect("config")).await;
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
                // 服务器已收到完整创建请求，但响应前连接断开；不能擅自再创建一次。
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
        };
        let result = api.post(&"c".repeat(26), "", "仅创建一次", &[]).await;
        server.abort();
        let _ = server.await;
        let error = result.expect_err("响应丢失应明确失败");
        assert!(!error.contains("synthetic-token"));
        let bodies = bodies.lock().expect("bodies");
        assert_eq!(bodies.len(), 1);
        let body: Value = serde_json::from_slice(&bodies[0]).expect("JSON");
        assert_eq!(body["message"], "仅创建一次");
    }

    #[tokio::test]
    #[ignore = "仅在明确授权的测试频道运行，保留合成长代码线程，不调用模型"]
    async fn live_long_code_is_losslessly_reassembled() {
        let site = std::env::var("CCCC_MM_TEST_SITE").expect("测试站点");
        let channel = std::env::var("CCCC_MM_TEST_CHANNEL").expect("测试频道");
        assert!(valid_id(&channel));
        let token =
            std::fs::read_to_string(std::env::var("CCCC_MM_TEST_TOKEN_FILE").expect("凭据文件"))
                .expect("凭据");
        let config = json!({"mattermost_url":site,"bot_token":token.trim()});
        let api = MattermostApi::authenticate(config.as_object().expect("config"))
            .await
            .expect("api");
        let (_temp, home, group) = scope();
        let root = api
            .post(
                &channel,
                "",
                &format!("长代码分段验收 {group}（合成协议，非模型输出）"),
                &[],
            )
            .await
            .expect("root");
        let text = format!(
            "```rust\n{}\n```\n[示例链接](https://example.com)\n结束🙂",
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
            "长代码验收 root={root} chunks={} chars={} posts={ids:?}",
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
            let error = fixture.api.socket().await.expect_err("应拒绝连接");
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("test-token"));
        }
    }

    #[tokio::test]
    async fn disconnected_socket_reconnects_and_clears_persisted_error() {
        let fixture = fixture().await;
        fixture.state.ws_mode.store(3, Ordering::SeqCst);
        let (_temp, home, group) = scope();
        let store = GroupStore::new(home.clone()).expect("store");
        let socket = fixture.api.socket().await.expect("首次连接");
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
        checked.expect("应记录断线并在重连后清除错误");
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
                .send_target(&target, &stream(&group, "start", "首帖失败"))
                .await
                .is_err()
        );
        fixture.state.fail_create.store(false, Ordering::Relaxed);
        let mut final_event = stream(&group, "end", "完整结论");
        final_event.kind = "chat.message".into();
        sender
            .send_target(&target, &final_event)
            .await
            .expect("首帖失败后兜底");
        sender
            .send_target(&target, &stream(&group, "start", "新流"))
            .await
            .expect("新流");
        fixture.state.fail_edit.store(true, Ordering::Relaxed);
        assert!(
            sender
                .send_target(&target, &stream(&group, "update", "更新失败"))
                .await
                .is_err()
        );
        sender
            .send_target(&target, &final_event)
            .await
            .expect("更新失败后兜底");
        let posts = fixture.state.posts.lock().expect("posts");
        assert_eq!(posts.len(), 3);
        assert_eq!(posts[0]["message"], "**reviewer**\n\n完整结论");
        assert_eq!(posts[2]["message"], posts[0]["message"]);
    }

    #[tokio::test]
    #[ignore = "仅在明确授权的测试站点和频道运行，会保留三条协议测试帖子"]
    async fn live_stream_updates_main_and_thread_without_duplicate_final() {
        let site = std::env::var("CCCC_MM_TEST_SITE").expect("测试站点");
        let channel = std::env::var("CCCC_MM_TEST_CHANNEL").expect("测试频道");
        assert!(valid_id(&channel));
        let token_path = std::env::var("CCCC_MM_TEST_TOKEN_FILE").expect("测试 Bot 凭据文件");
        let token = std::fs::read_to_string(token_path).expect("读取测试 Bot 凭据");
        let config = json!({"mattermost_url":site,"bot_token":token.trim()});
        let api = MattermostApi::authenticate(config.as_object().expect("配置"))
            .await
            .expect("测试 Bot 身份");
        let (_temp, home, group) = scope();
        let label = format!("协议流式验收 {group}（非模型输出）");
        let root = api.post(&channel, "", &label, &[]).await.expect("测试根帖");
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
        for (op, suffix) in [("start", "开始"), ("update", "处理中"), ("end", "完成🙂")] {
            let text = format!("{label}：{suffix}");
            for target in &targets {
                sender
                    .send_target(target, &stream(&group, op, &text))
                    .await
                    .expect("流式投递");
            }
            let posts = api
                .json(
                    Method::GET,
                    &format!("channels/{channel}/posts?per_page=20"),
                    None,
                )
                .await
                .expect("读取测试帖子");
            for (index, target) in targets.iter().enumerate() {
                let expected = format!("**reviewer**\n\n{text}");
                let matching: Vec<_> = posts["posts"]
                    .as_object()
                    .expect("帖子列表")
                    .values()
                    .filter(|post| {
                        post["message"] == expected && post["root_id"] == target.thread_id
                    })
                    .collect();
                assert_eq!(matching.len(), 1, "每个目标应只有一个当前版本");
                let id = field(matching[0], "id").to_owned();
                if op == "start" {
                    post_ids.push(id);
                } else {
                    assert_eq!(id, post_ids[index]);
                }
            }
        }
        let mut final_event = stream(&group, "end", &format!("{label}：完成🙂"));
        final_event.kind = "chat.message".into();
        for target in &targets {
            sender
                .send_target(target, &final_event)
                .await
                .expect("最终消息去重");
        }
        let posts = api
            .json(
                Method::GET,
                &format!("channels/{channel}/posts?per_page=20"),
                None,
            )
            .await
            .expect("读取最终帖子");
        let count = posts["posts"]
            .as_object()
            .expect("帖子列表")
            .values()
            .filter(|post| field(post, "message").contains(&label))
            .count();
        assert_eq!(count, 3, "只保留测试根帖与两个已编辑的目标帖");
    }

    #[tokio::test]
    async fn verifies_bot_and_websocket_with_site_subpath() {
        let fixture = fixture().await;
        let config = json!({"mattermost_url":fixture.api.site,"bot_token":"test-token"});
        let api =
            MattermostApi::authenticate(config.as_object().expect("Mattermost test operation"))
                .await
                .expect("Mattermost test operation");
        assert_eq!(api.bot_id, "b".repeat(26));
        let mut socket = api.socket().await.expect("Mattermost test operation");
        socket.close(None).await.expect("Mattermost test operation");
        let mut invalid = config.clone();
        invalid["bot_token"] = json!("wrong-token");
        let error =
            MattermostApi::authenticate(invalid.as_object().expect("Mattermost test operation"))
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
        let end = stream(&group, "end", "结论🙂");
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
        assert_eq!(posts[2]["message"], "**reviewer**\n\n结论🙂");
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
            .send_target(&target, &stream(&group, "end", "初稿"))
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
        let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"t".repeat(26),"message":"@cccc_bot 看附件","type":"","file_ids":["f".repeat(26)]});
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
        // 提交失败/取消释放锁后，无关完成也不得清除失败请求以外的反应。
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
        // 超过三次心跳周期且队列无人消费：不得因本地背压误报远端超时。
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
        let post = json!({"id":"p".repeat(26),"user_id":"u".repeat(26),"channel_id":"c".repeat(26),"root_id":"","message":"@cccc_bot 看附件","type":"","file_ids":["f".repeat(26)]});
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
            crate::ledger_event_hub::LedgerEventHub::new(home),
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
            assert!(field(&posts[0], "message").contains("未能交给 CCCC"));
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
}
