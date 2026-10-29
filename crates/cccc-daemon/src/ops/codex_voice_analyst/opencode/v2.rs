//! OpenCode V2's public HTTP/SSE API and an attached native TUI share one
//! CCCC-owned backend. V2 ACP owns a separate private server and cannot host
//! this topology. No provider database, global server or private socket is used.
use super::super::{AnalystEvent, ManagedProtocol, SessionPurpose, native_input, process};
use super::{LaunchedOpenCode, PreparedOpenCode, STARTUP_TIMEOUT, lifecycle};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, BufRead};
use std::path::Path;
use std::process::ChildStdin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, watch};

mod events;
#[cfg(test)]
mod tests;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(in crate::ops::codex_voice_analyst) struct OpenCodeClient {
    inner: Arc<Inner>,
    stop: watch::Sender<bool>,
    lease: Option<ChildStdin>,
}

struct Inner {
    http: reqwest::Client,
    endpoint: String,
    session_id: String,
    generation: String,
    events: broadcast::Sender<AnalystEvent>,
    state: Mutex<events::State>,
    running: AtomicBool,
}

// A cancelled HTTP future can already have admitted input. Release the owner,
// rather than leave its unresolved correlation barrier looking permanently busy.
struct PendingAdmission<'a> {
    client: &'a OpenCodeClient,
    confirmed: bool,
}

impl Drop for PendingAdmission<'_> {
    fn drop(&mut self) {
        if !self.confirmed {
            self.client.inner.running.store(false, Ordering::Release);
            let _ = self.client.stop.send(true);
            self.client.inner.publish(
                json!({"method":"cccc/managedAgent/disconnected","params":{
                    "threadId":self.client.inner.session_id,
                    "reason":"OpenCode V2 input admission is unresolved; pending input will not be replayed",
                }}),
                None,
            );
        }
    }
}

impl Inner {
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.endpoint))
            .timeout(REQUEST_TIMEOUT)
    }

    async fn json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> io::Result<Value> {
        let mut request = self.request(method, path);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|_| {
            io::Error::other("OpenCode V2 request outcome is unknown; it will not be replayed")
        })?;
        let status = response.status();
        if !status.is_success() {
            // Response bodies and URLs can carry provider or MCP credentials.
            return Err(io::Error::other(format!(
                "OpenCode V2 request was rejected (HTTP {})",
                status.as_u16()
            )));
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        response.json().await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "OpenCode V2 returned invalid JSON",
            )
        })
    }

    fn publish(&self, message: Value, delegation: Option<String>) {
        let _ = self.events.send(AnalystEvent {
            generation: self.generation.clone(),
            message,
            requested_delegation_id: delegation,
        });
    }
}

impl OpenCodeClient {
    pub(in crate::ops::codex_voice_analyst) fn subscribe(
        &self,
    ) -> broadcast::Receiver<AnalystEvent> {
        self.inner.events.subscribe()
    }
    pub(in crate::ops::codex_voice_analyst) fn running(&self) -> bool {
        self.inner.running.load(Ordering::Acquire)
    }
    pub(in crate::ops::codex_voice_analyst) async fn close(&self) {
        let _ = self.stop.send(true);
        self.inner.running.store(false, Ordering::Release);
    }
    pub(in crate::ops::codex_voice_analyst) async fn register_native_input(
        &self,
        delegation: &str,
        text: &str,
    ) -> io::Result<()> {
        native_input::register(
            &mut self.inner.state.lock().await.native,
            delegation.into(),
            text.into(),
        )
    }
    pub(in crate::ops::codex_voice_analyst) async fn forget_native_input(
        &self,
        delegation: &str,
    ) -> io::Result<()> {
        native_input::forget(&mut self.inner.state.lock().await.native, delegation);
        Ok(())
    }
    pub(in crate::ops::codex_voice_analyst) async fn start_prompt(
        &self,
        delegation: &str,
        text: &str,
    ) -> io::Result<String> {
        // Let OpenCode generate its ordered message ID. A UUID with a msg_
        // prefix is not an OpenCode ID and can sort behind existing history.
        // Keep observation serialized until the durable admission identifies
        // this request; no model output or completion is inferred from the RPC.
        let mut state = self.inner.state.lock().await;
        if !self.running() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "OpenCode V2 lifecycle is unavailable",
            ));
        }
        if state.active.is_some() || state.controlled.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "OpenCode is processing an existing turn",
            ));
        }
        state.controlled = Some((String::new(), delegation.into()));
        let mut admission = PendingAdmission {
            client: self,
            confirmed: false,
        };
        let response = self
            .inner
            .json(
                reqwest::Method::POST,
                &format!("/api/session/{}/prompt", self.inner.session_id),
                Some(&json!({"text":text,"delivery":"queue"})),
            )
            .await?;
        let id = response["data"]["id"]
            .as_str()
            .filter(|id| id.starts_with("msg_"))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "OpenCode V2 returned no admitted message ID",
                )
            })?
            .to_owned();
        if response["data"]["sessionID"] != self.inner.session_id
            || response["data"]["delivery"] != "queue"
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "OpenCode V2 did not acknowledge the queued input",
            ));
        }
        state.controlled = Some((id.clone(), delegation.into()));
        admission.confirmed = true;
        Ok(id)
    }

    pub(in crate::ops::codex_voice_analyst) async fn cancel(
        &self,
        turn_id: &str,
    ) -> io::Result<()> {
        let state = self.inner.state.lock().await;
        if state.active.as_ref().is_none_or(|turn| turn.id != turn_id) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "OpenCode turn is no longer active",
            ));
        }
        // /interrupt only accepts a session ID. It can race with completion and
        // stop a newer native turn, so do not substitute it for exact-turn control.
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "OpenCode V2 does not support fenced turn interruption; interrupt in its native TUI, or stop the Actor's owned backend",
        ))
    }

    #[cfg(test)]
    pub(in crate::ops::codex_voice_analyst) fn publish_for_test(&self, event: AnalystEvent) {
        let _ = self.inner.events.send(event);
    }
}

impl Drop for OpenCodeClient {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.lease.take();
    }
}

pub(super) async fn launch(
    prepared: PreparedOpenCode,
    cwd: &Path,
    mut environment: BTreeMap<String, String>,
    generation: &str,
    purpose: SessionPurpose,
    resume: Option<&str>,
    mcp: Value,
) -> io::Result<LaunchedOpenCode> {
    if prepared.acp_arguments.iter().any(|arg| arg == "--pure") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "OpenCode V2 no longer supports --pure; remove it from the runtime command",
        ));
    }
    let agent = match purpose {
        SessionPurpose::VoiceAnalyst => Some((
            super::command::VOICE_ANALYST_AGENT,
            super::super::launch::ANALYST_INSTRUCTIONS,
        )),
        SessionPurpose::VoiceSecretary => Some((
            super::command::VOICE_SECRETARY_AGENT,
            super::super::launch_secretary::INSTRUCTIONS,
        )),
        _ => None,
    };
    let content = configuration(
        environment
            .get("OPENCODE_CONFIG_CONTENT")
            .map(String::as_str),
        mcp,
        agent,
    )?;
    environment.insert(
        "OPENCODE_CONFIG_CONTENT".into(),
        serde_json::to_string(&content)?,
    );
    let password = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    environment.insert("OPENCODE_PASSWORD".into(), password.clone());
    // serve --stdio removes both credentials from tool subprocess environments
    // and exits when CCCC releases stdin. Never use the user's shared service.
    let mut command = prepared.launch_prefix.clone();
    command.extend([
        "serve".into(),
        "--stdio".into(),
        "--hostname".into(),
        "127.0.0.1".into(),
        "--port".into(),
        "0".into(),
    ]);
    command.extend(prepared.acp_arguments.clone());
    let (owner, stdin, stdout) = process::spawn_piped_quiet(&command, cwd, &environment)?;
    let owner = Arc::new(owner);
    let initialized = async {
        let endpoint = stdio_endpoint(stdout).await?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            lifecycle::basic_authorization("opencode", &password)
                .parse()
                .map_err(io::Error::other)?,
        );
        let http = reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .build()
            .map_err(io::Error::other)?;
        let anonymous = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(io::Error::other)?;
        if anonymous
            .get(format!("{endpoint}/api/info"))
            .send()
            .await
            .map_err(|_| io::Error::other("OpenCode V2 authentication probe failed"))?
            .status()
            != reqwest::StatusCode::UNAUTHORIZED
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "OpenCode V2 backend did not enforce CCCC-owned authentication",
            ));
        }
        let (events, _) = broadcast::channel(1024);
        let mut inner = Inner {
            http,
            endpoint,
            session_id: String::new(),
            generation: generation.into(),
            events,
            state: Mutex::new(events::State::default()),
            running: AtomicBool::new(true),
        };
        let info = inner.json(reqwest::Method::GET, "/api/info", None).await?;
        if info["pid"].as_u64() != owner.id().map(u64::from) {
            return Err(io::Error::other(
                "OpenCode V2 backend does not belong to the launched process",
            ));
        }
        let desired_agent = agent.map(|(name, _)| name).or(prepared.agent.as_deref());
        let model = prepared.model.as_deref().map(model_ref).transpose()?;
        let session = if let Some(id) = resume {
            let session = inner
                .json(reqwest::Method::GET, &format!("/api/session/{id}"), None)
                .await?;
            if session["data"]["id"] != id
                || session["data"]["location"]["directory"] != cwd.to_string_lossy().as_ref()
            {
                return Err(io::Error::other(
                    "OpenCode V2 saved session has a different workspace or identity",
                ));
            }
            if let Some(model) = &model {
                inner
                    .json(
                        reqwest::Method::POST,
                        &format!("/api/session/{id}/model"),
                        Some(&json!({"model":model})),
                    )
                    .await?;
            }
            if let Some(agent) = desired_agent {
                inner
                    .json(
                        reqwest::Method::POST,
                        &format!("/api/session/{id}/agent"),
                        Some(&json!({"agent":agent})),
                    )
                    .await?;
            }
            session
        } else {
            let mut body = json!({"location":{"directory":cwd},"title":"CCCC managed session"});
            if let Some(model) = model {
                body["model"] = model;
            }
            if let Some(agent) = desired_agent {
                body["agent"] = json!(agent);
            }
            inner
                .json(reqwest::Method::POST, "/api/session", Some(&body))
                .await?
        };
        let id = session["data"]["id"]
            .as_str()
            .filter(|id| super::super::super::runtime_session::valid_opencode_session_id(id))
            .ok_or_else(|| io::Error::other("OpenCode V2 returned an invalid session ID"))?
            .to_owned();
        inner.session_id = id.clone();
        // A running server can still have an empty MCP catalog while its
        // configured servers connect. Do not spend a model turn discovering
        // that the Actor's required CCCC tools were not loaded yet.
        tokio::time::timeout(STARTUP_TIMEOUT, async {
            loop {
                let response=inner.http.get(format!("{}/api/mcp",inner.endpoint))
                    .query(&[("location[directory]",cwd.to_string_lossy().as_ref())])
                    .timeout(Duration::from_secs(2)).send().await
                    .map_err(|_|io::Error::other("OpenCode V2 CCCC MCP readiness check failed"))?;
                if !response.status().is_success() {return Err(io::Error::other("OpenCode V2 rejected the CCCC MCP readiness check"));}
                let status=response.json::<Value>().await.map_err(|_|io::Error::other("OpenCode V2 returned invalid MCP status"))?;
                if let Some(server)=status["data"].as_array().into_iter().flatten().find(|server|server["name"]=="cccc") {
                    match server["status"]["status"].as_str() {
                        Some("connected")=>return Ok(()),
                        Some("failed"|"disabled"|"needs_auth")=>return Err(io::Error::other("OpenCode V2 could not connect the Actor's CCCC MCP server; check the configured CCCC executable and permissions")),
                        _=>{}
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }).await.map_err(|_|io::Error::new(io::ErrorKind::TimedOut,"OpenCode V2 CCCC MCP server did not become ready"))??;
        // Receive server.connected before admitting any input; HTTP headers
        // alone do not prove the volatile event listener has been registered.
        let mut response = tokio::time::timeout(
            STARTUP_TIMEOUT,
            inner
                .http
                .get(format!("{}/api/event", inner.endpoint))
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send(),
        )
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "OpenCode V2 event stream connection timed out"))?
            .map_err(|_| io::Error::other("OpenCode V2 event stream is unavailable"))?;
        if !response.status().is_success()
            || !response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("text/event-stream"))
        {
            return Err(io::Error::other(
                "OpenCode V2 did not return an event stream",
            ));
        }
        let mut buffer = lifecycle::SseLineBuffer::new(16 * 1024 * 1024);
        tokio::time::timeout(STARTUP_TIMEOUT, async {
            loop {
                let chunk = response
                    .chunk()
                    .await
                    .map_err(|_| {
                        io::Error::other("OpenCode V2 event stream disconnected before readiness")
                    })?
                    .ok_or_else(|| {
                        io::Error::other("OpenCode V2 event stream ended before readiness")
                    })?;
                let values = decode_chunk(&mut buffer, &chunk)?;
                if values.iter().any(|v| v["type"] == "server.connected") {
                    return Ok::<_, io::Error>(());
                }
            }
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "OpenCode V2 event listener did not become ready",
            )
        })??;
        let mut tui = prepared.launch_prefix;
        tui.extend([
            "--server".into(),
            inner.endpoint.clone(),
            "--session".into(),
            id.clone(),
        ]);
        tui.extend(prepared.tui_arguments);
        tui.push(cwd.to_string_lossy().into_owned());
        let inner = Arc::new(inner);
        let (stop, stopped) = watch::channel(false);
        tokio::spawn(observe(Arc::clone(&inner), response, buffer, stopped));
        Ok((
            OpenCodeClient {
                inner,
                stop,
                lease: Some(stdin),
            },
            id,
            tui,
        ))
    }
    .await;
    match initialized {
        Ok((client, id, tui)) => Ok(LaunchedOpenCode {
            protocol: ManagedProtocol::OpenCode(client),
            process: owner,
            session_id: id,
            tui_command: tui,
            environment,
            resumed: resume.is_some(),
        }),
        Err(error) => {
            let _ = owner.stop();
            Err(error)
        }
    }
}

fn configuration(
    existing: Option<&str>,
    mcp: Value,
    agent: Option<(&str, &str)>,
) -> io::Result<Value> {
    let mut content = match existing {
        Some(text) => serde_json::from_str(text).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "OPENCODE_CONFIG_CONTENT must contain valid JSON",
            )
        })?,
        None => json!({}),
    };
    if !content.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "OpenCode inline configuration must be an object",
        ));
    }
    let mut command = vec![mcp["command"].clone()];
    command.extend(mcp["args"].as_array().into_iter().flatten().cloned());
    let environment = mcp["env"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e["name"].as_str()?.to_owned(), e["value"].clone())))
        .collect::<serde_json::Map<_, _>>();
    for path in ["/mcp", "/mcp/servers", "/agents"] {
        if content
            .pointer(path)
            .is_some_and(|value| !value.is_null() && !value.is_object())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("OpenCode inline {path} configuration must be an object"),
            ));
        }
    }
    content["mcp"]["servers"]["cccc"] =
        json!({"type":"local","command":command,"environment":environment,"codemode":false});
    if let Some((name, instructions)) = agent {
        content["agents"][name] = json!({"mode":"primary","description":"CCCC managed voice assistant","system":instructions});
    }
    Ok(content)
}

fn model_ref(value: &str) -> io::Result<Value> {
    let (base, variant) = value
        .split_once('#')
        .map_or((value, None), |(base, variant)| (base, Some(variant)));
    let (provider, id) = base
        .split_once('/')
        .filter(|(p, id)| !p.is_empty() && !id.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "OpenCode model must use provider/model format",
            )
        })?;
    let mut model = json!({"providerID":provider,"id":id});
    if let Some(variant) = variant {
        model["variant"] = json!(variant);
    }
    Ok(model)
}

async fn stdio_endpoint(stdout: std::process::ChildStdout) -> io::Result<String> {
    let (ready, receive) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("cccc-opencode-v2-stdio".into())
        .spawn(move || {
            let mut reader = io::BufReader::new(stdout);
            let mut bytes = Vec::new();
            let result = (|| {
                use io::Read;
                reader.by_ref().take(16_384).read_until(b'\n', &mut bytes)?;
                let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
                    io::Error::other("OpenCode V2 did not return its stdio endpoint")
                })?;
                let endpoint = value["url"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("OpenCode V2 stdio endpoint is missing"))?;
                let url = url::Url::parse(endpoint).map_err(io::Error::other)?;
                if url.scheme() != "http"
                    || url.host_str() != Some("127.0.0.1")
                    || url.port().is_none_or(|p| p == 0)
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "OpenCode V2 requires an uncredentialed loopback HTTP endpoint",
                    ));
                }
                Ok(endpoint.trim_end_matches('/').to_owned())
            })();
            let _ = ready.send(result);
            let _ = io::copy(&mut reader, &mut io::sink());
        })?;
    tokio::time::timeout(STARTUP_TIMEOUT, receive)
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "OpenCode V2 stdio startup timed out",
            )
        })?
        .map_err(|_| io::Error::other("OpenCode V2 stdio closed during startup"))?
}

fn decode_chunk(buffer: &mut lifecycle::SseLineBuffer, bytes: &[u8]) -> io::Result<Vec<Value>> {
    let mut values = Vec::new();
    for fragment in bytes.split_inclusive(|byte| *byte == b'\n') {
        if let Some(value) = buffer.push(fragment)? {
            values.push(if let Some(encoded) = value.as_str() {
                serde_json::from_str(encoded).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid OpenCode V2 encoded event",
                    )
                })?
            } else {
                value
            });
        }
    }
    Ok(values)
}

async fn observe(
    inner: Arc<Inner>,
    mut response: reqwest::Response,
    mut buffer: lifecycle::SseLineBuffer,
    mut stopped: watch::Receiver<bool>,
) {
    let result:io::Result<()>=async {
        loop {
            let chunk=tokio::select! {
                _=stopped.changed()=>return Ok(()),
                chunk=response.chunk()=>chunk.map_err(|_|io::Error::other("OpenCode V2 lifecycle stream disconnected"))?.ok_or_else(||io::Error::other("OpenCode V2 lifecycle stream ended"))?,
            };
            for event in decode_chunk(&mut buffer,&chunk)? {
                inner.state.lock().await.observe(&inner,&event)?;
                if event["type"]=="permission.requested" && event["data"]["sessionID"]==inner.session_id {
                    let id=event["data"]["id"].as_str().ok_or_else(||io::Error::other("OpenCode permission has no ID"))?;
                    inner.json(reqwest::Method::POST,&format!("/api/session/{}/permission/{id}/reply",inner.session_id),Some(&json!({"decision":"once"}))).await?;
                }
            }
        }
    }.await;
    inner.running.store(false, Ordering::Release);
    if result.is_err() {
        inner.publish(json!({"method":"cccc/managedAgent/disconnected","params":{"threadId":inner.session_id,"reason":"OpenCode V2 lifecycle was lost; pending input will not be replayed"}}),None);
    }
}
