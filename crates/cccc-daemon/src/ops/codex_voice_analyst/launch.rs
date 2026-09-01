use super::*;
use cccc_core::GroupStore;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Duration;

const ANALYST_INSTRUCTIONS: &str = r#"You are the Voice Analyst behind CCCC Realtime Voice. In delegated speech, references such as 'the analyst', 'ask the analyst', or 'have the analyst check' refer to you: perform that investigation directly with your own tools. Never use Codex collaboration or sub-agent tools in this role. When additional execution is genuinely needed, coordinate an existing CCCC Group Foreman or peer through CCCC tools instead of creating an untracked second analyst. Investigate material claims with tools before answering.

The host supplies one bootstrap Group and canonical repository root because Codex requires a concrete initial working directory. They do not define the scope or identity of this global Voice assistant. When the user asks about all Groups or names another Group, use cccc_group list/resolve and an explicit group_id to query live daemon state. Never infer live state from CCCC_HOME directories or describe one Group snapshot as global state. Cross-Group CCCC actions require an explicit, unambiguous target. For repository or shell investigation outside the bootstrap root, first resolve the intended Group and attached root through CCCC, then operate on that explicit target.

Use existing CCCC tools when live Group facts or durable Actor work are needed; hand off only when the requested outcome genuinely requires durable execution rather than your own investigation. Never claim that work was accepted unless the tool returned durable task or message facts. Keep progress substantive and the final concise and evidence-backed for speech; detailed work remains visible in Codex."#;

impl AnalystSession {
    pub(crate) async fn launch(home: &HomeLayout, config: LaunchConfig) -> io::Result<Self> {
        let binding = bind_scope(home, &config.group_id, &config.root)?;
        let codex_executable = match config.codex_executable {
            Some(path) if path.is_file() => path,
            Some(path) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Codex executable does not exist: {}", path.display()),
                ));
            }
            None => cccc_runtime::resolve_executable_in_path("codex", None).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "Codex CLI is not installed or not in PATH",
                )
            })?,
        };
        let mut command = app_server_command(&codex_executable, config.profile.as_deref());
        let mut env = BTreeMap::new();
        if !super::super::codex_mcp::configure_mcp_only(
            home,
            &binding.group_id,
            "user",
            &mut command,
            &mut env,
        ) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "CCCC executable is unavailable for Voice Analyst MCP binding",
            ));
        }
        let (process, lines) = process::spawn_app_server(&command, &binding.root, &env)?;
        let process = Arc::new(process);
        let endpoint = process::wait_for_endpoint(lines).await?;
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let result = Self::connect(ConnectConfig {
            binding,
            generation,
            endpoint,
            codex_executable,
            model: config.model,
            resume_thread_id: config.resume_thread_id,
            process: Some(Arc::clone(&process)),
            delegations: HashMap::new(),
        })
        .await;
        if result.is_err() {
            let _ = process.stop();
        }
        result
    }

    pub(super) async fn connect(config: ConnectConfig) -> io::Result<Self> {
        let ConnectConfig {
            binding,
            generation,
            endpoint,
            codex_executable,
            model,
            resume_thread_id,
            process,
            delegations,
        } = config;
        process::validate_loopback_endpoint(&endpoint)?;
        let socket = protocol::connect_with_retry(&endpoint).await?;
        let protocol = ProtocolClient::new(socket, generation.clone());
        protocol
            .request(
                "initialize",
                json!({
                    "clientInfo":{"name":"cccc-voice-analyst","version":env!("CARGO_PKG_VERSION")},
                    "capabilities":{"experimentalApi":true}
                }),
                Duration::from_secs(10),
            )
            .await?;
        let mut params = json!({
            "cwd": binding.root,
            "developerInstructions": ANALYST_INSTRUCTIONS,
            "approvalPolicy":"never",
            "sandbox":"danger-full-access",
        });
        if let Some(model) = model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            params["model"] = json!(model);
        }
        let requested_thread_id = resume_thread_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let thread_materialized = requested_thread_id.is_some();
        let (method, mut params) = if let Some(thread_id) = requested_thread_id {
            params["threadId"] = json!(thread_id);
            ("thread/resume", params)
        } else {
            ("thread/start", params)
        };
        if method == "thread/start" {
            // Stock Codex TUI can resume legacy history, which lets Web attach to this thread.
            params["historyMode"] = json!("legacy");
        }
        let started = protocol
            .request(method, params, Duration::from_secs(20))
            .await?;
        let thread_id = started
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| io::Error::other("Codex app-server returned an empty thread id"))?
            .to_owned();
        if requested_thread_id.is_some_and(|requested| requested != thread_id) {
            return Err(io::Error::other(
                "Codex app-server resumed a different Voice Analyst thread",
            ));
        }
        Ok(Self {
            binding,
            generation,
            endpoint,
            thread_id,
            codex_executable,
            protocol,
            process,
            thread_materialized: AtomicBool::new(thread_materialized),
            delegations: tokio::sync::Mutex::new(delegations),
        })
    }

    #[cfg(test)]
    pub(crate) async fn connect_for_test(
        binding: ScopeBinding,
        generation: String,
        endpoint: String,
        codex_executable: PathBuf,
        model: Option<&str>,
    ) -> io::Result<Self> {
        Self::connect(ConnectConfig {
            binding,
            generation,
            endpoint,
            codex_executable,
            model: model.map(str::to_owned),
            resume_thread_id: None,
            process: None,
            delegations: HashMap::new(),
        })
        .await
    }

    pub(crate) fn binding(&self) -> &ScopeBinding {
        &self.binding
    }

    pub(crate) fn generation(&self) -> &str {
        &self.generation
    }

    #[cfg(test)]
    pub(super) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<AnalystEvent> {
        self.protocol.subscribe()
    }

    #[cfg(test)]
    pub(crate) fn publish_event_for_test(&self, message: Value) {
        let _ = self.protocol.events.send(AnalystEvent {
            generation: self.generation.clone(),
            message,
        });
    }

    pub(crate) fn tui_command(&self) -> Vec<String> {
        vec![
            self.codex_executable.to_string_lossy().into_owned(),
            "--remote".into(),
            self.endpoint.clone(),
            "resume".into(),
            self.thread_id.clone(),
            "--no-alt-screen".into(),
        ]
    }

    pub(crate) fn tui_ready(&self) -> bool {
        self.thread_materialized.load(AtomicOrdering::Acquire)
    }

    pub(crate) fn mark_thread_materialized(&self) {
        self.thread_materialized
            .store(true, AtomicOrdering::Release);
    }
}

pub(super) fn bind_scope(
    home: &HomeLayout,
    group_id: &str,
    root: &Path,
) -> io::Result<ScopeBinding> {
    let group_id = required_value(group_id, "group_id")?;
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Voice Analyst root must be a directory",
        ));
    }
    let group = GroupStore::new(home.clone())?.load(group_id)?;
    let attached = group.scopes.iter().any(|scope| {
        Path::new(&scope.url)
            .canonicalize()
            .is_ok_and(|candidate| candidate == root)
    });
    if !attached {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Voice Analyst root is not attached to its bootstrap Group",
        ));
    }
    Ok(ScopeBinding {
        group_id: group_id.to_owned(),
        root,
    })
}

pub(super) fn app_server_command(codex_executable: &Path, profile: Option<&str>) -> Vec<String> {
    let mut command = vec![
        codex_executable.to_string_lossy().into_owned(),
        "--dangerously-bypass-approvals-and-sandbox".into(),
        "--search".into(),
        "-c".into(),
        "approval_policy=\"never\"".into(),
        "-c".into(),
        "sandbox_mode=\"danger-full-access\"".into(),
    ];
    if let Some(profile) = profile.map(str::trim).filter(|value| !value.is_empty()) {
        command.extend(["--profile".into(), profile.into()]);
    }
    command.extend([
        "app-server".into(),
        "--listen".into(),
        "ws://127.0.0.1:0".into(),
    ]);
    command
}
