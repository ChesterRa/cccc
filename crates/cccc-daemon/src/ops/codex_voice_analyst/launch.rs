use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::Duration;

const ANALYST_INSTRUCTIONS: &str = r#"You are the Voice Analyst behind CCCC Realtime Voice. In delegated speech, references such as 'the analyst', 'ask the analyst', or 'have the analyst check' refer to you: perform that investigation directly with your own tools. Never use Codex collaboration or sub-agent tools in this role. When additional execution is genuinely needed, coordinate an existing CCCC Group Foreman or peer through CCCC tools instead of creating an untracked second analyst. Investigate material claims with tools before answering.

The host starts you in a neutral CCCC-owned working directory. It is not a Working Group, repository scope, or implicit target. Every CCCC operation concerning a Group, Actor, task, message, ledger, or repository must use an explicit group_id and any required target identity. When the user asks about all Groups or names another Group, use CCCC tools to list or resolve live state. Never infer live state from CCCC_HOME directories or describe one Group snapshot as global state. Before repository investigation, resolve the intended Group and attached root, read the applicable repository instructions, and operate only on that explicit target. Delegate repository modification or durable work to the existing Group Foreman or peer instead of treating this neutral cwd as the project.

Use existing CCCC tools when live Group facts or durable Actor work are needed; hand off only when the requested outcome genuinely requires durable execution rather than your own investigation. Never claim that work was accepted unless the tool returned durable task or message facts. Keep progress substantive and the final concise and evidence-backed for speech; detailed work remains visible in Codex."#;

impl AnalystSession {
    pub(crate) async fn launch(home: &HomeLayout, config: LaunchConfig) -> io::Result<Self> {
        let binding = bind_workspace(&config.workdir)?;
        cccc_core::codex_voice_settings::validate_private_environment(&config.environment)?;
        if config.runtime != cccc_contracts::ActorRuntime::Codex {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "Voice Analyst has no adapter for the {:?} runtime",
                    config.runtime
                ),
            ));
        }
        let mut env = config.environment;
        let prepared = super::launch_command::prepare(&config.command, &env)?;
        let mut command = prepared.app_server;
        if !super::super::codex_mcp::configure_global_user_mcp(home, &mut command, &mut env) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "CCCC executable is unavailable for Voice Analyst MCP binding",
            ));
        }
        Self::launch_prepared(
            binding,
            prepared.remote_tui_prefix,
            command,
            env,
            config.resume_thread_id,
            SessionPurpose::VoiceAnalyst,
        )
        .await
    }

    pub(crate) async fn launch_actor(
        home: &HomeLayout,
        config: ActorLaunchConfig,
    ) -> io::Result<Self> {
        let binding = bind_workspace(&config.workdir)?;
        let mut env = config.environment;
        let prepared = super::launch_command::prepare(&config.command, &env)?;
        let session_command = prepared.app_server.clone();
        let resume_thread_id = super::super::runtime_session::prepare_codex_app_thread(
            home,
            &config.group_id,
            &config.actor_id,
            &binding.root,
            &session_command,
            &prepared.model,
        )?;
        let mut command = prepared.app_server;
        if !super::super::codex_mcp::configure_mcp_only(
            home,
            &config.group_id,
            &config.actor_id,
            &mut command,
            &mut env,
        ) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "CCCC executable is unavailable for Codex Actor MCP binding",
            ));
        }
        let session = Self::launch_prepared(
            binding,
            prepared.remote_tui_prefix,
            command,
            env,
            resume_thread_id,
            SessionPurpose::Actor,
        )
        .await?;
        if let Err(error) = super::super::runtime_session::record_codex_app_thread(
            home,
            &config.group_id,
            &config.actor_id,
            &config.workdir,
            &session_command,
            super::super::runtime_session::CodexAppThread {
                id: session.thread_id(),
                resumed: session.thread_resumed,
                runner: config.runner,
            },
        ) {
            tracing::warn!(
                %error,
                group_id = %config.group_id,
                actor_id = %config.actor_id,
                "failed to persist Codex Actor app-server thread"
            );
        }
        Ok(session)
    }

    async fn launch_prepared(
        binding: WorkspaceBinding,
        remote_tui_prefix: Vec<String>,
        command: Vec<String>,
        env: BTreeMap<String, String>,
        resume_thread_id: Option<String>,
        purpose: SessionPurpose,
    ) -> io::Result<Self> {
        let (process, lines) = process::spawn_app_server(&command, &binding.root, &env)?;
        let process = Arc::new(process);
        let endpoint = process::wait_for_endpoint(lines).await?;
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let result = Self::connect(ConnectConfig {
            binding,
            generation,
            endpoint,
            remote_tui_prefix,
            environment: env,
            resume_thread_id,
            process: Some(Arc::clone(&process)),
            delegations: HashMap::new(),
            purpose,
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
            remote_tui_prefix,
            environment,
            resume_thread_id,
            process,
            delegations,
            purpose,
        } = config;
        process::validate_loopback_endpoint(&endpoint)?;
        let socket = protocol::connect_with_retry(&endpoint).await?;
        let protocol = ProtocolClient::new(socket, generation.clone());
        protocol
            .request(
                "initialize",
                json!({
                    "clientInfo":{"name":match purpose {
                        SessionPurpose::VoiceAnalyst => "cccc-voice-analyst",
                        SessionPurpose::Actor => "cccc-actor",
                    },"version":env!("CARGO_PKG_VERSION")},
                    "capabilities":{"experimentalApi":true}
                }),
                Duration::from_secs(10),
            )
            .await?;
        let mut params = json!({
            "cwd": binding.root,
            "approvalPolicy":"never",
            "sandbox":"danger-full-access",
        });
        match purpose {
            SessionPurpose::VoiceAnalyst => {
                params["developerInstructions"] = json!(ANALYST_INSTRUCTIONS);
            }
            SessionPurpose::Actor => {
                params["personality"] = json!("pragmatic");
            }
        }
        let requested_thread_id = resume_thread_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (started, thread_resumed) = if let Some(thread_id) = requested_thread_id {
            let mut resume_params = params.clone();
            resume_params["threadId"] = json!(thread_id);
            match protocol
                .request("thread/resume", resume_params, Duration::from_secs(20))
                .await
            {
                Ok(started) => (started, true),
                Err(error) if purpose == SessionPurpose::Actor => {
                    tracing::warn!(
                        %error,
                        thread_id,
                        "Codex Actor thread resume failed; starting one fresh thread"
                    );
                    params["historyMode"] = json!("legacy");
                    (
                        protocol
                            .request("thread/start", params, Duration::from_secs(20))
                            .await?,
                        false,
                    )
                }
                Err(error) => return Err(error),
            }
        } else {
            // Stock Codex TUI can resume legacy history, which lets Web attach to this thread.
            params["historyMode"] = json!("legacy");
            (
                protocol
                    .request("thread/start", params, Duration::from_secs(20))
                    .await?,
                false,
            )
        };
        let thread_id = started
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| io::Error::other("Codex app-server returned an empty thread id"))?
            .to_owned();
        if thread_resumed && requested_thread_id.is_some_and(|requested| requested != thread_id) {
            return Err(io::Error::other(
                "Codex app-server resumed a different thread",
            ));
        }
        Ok(Self {
            #[cfg(test)]
            binding,
            generation,
            endpoint,
            thread_id,
            remote_tui_prefix,
            environment,
            protocol,
            process,
            thread_materialized: AtomicBool::new(thread_resumed),
            thread_resumed,
            delegations: tokio::sync::Mutex::new(delegations),
        })
    }

    #[cfg(test)]
    pub(crate) async fn connect_for_test(
        binding: WorkspaceBinding,
        generation: String,
        endpoint: String,
        codex_executable: PathBuf,
    ) -> io::Result<Self> {
        Self::connect(ConnectConfig {
            binding,
            generation,
            endpoint,
            remote_tui_prefix: vec![codex_executable.to_string_lossy().into_owned()],
            environment: BTreeMap::new(),
            resume_thread_id: None,
            process: None,
            delegations: HashMap::new(),
            purpose: SessionPurpose::VoiceAnalyst,
        })
        .await
    }

    #[cfg(test)]
    pub(crate) fn binding(&self) -> &WorkspaceBinding {
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
            requested_delegation_id: None,
        });
    }

    pub(crate) fn tui_command(&self) -> Vec<String> {
        let mut command = self.remote_tui_prefix.clone();
        command.extend([
            "--remote".into(),
            self.endpoint.clone(),
            "resume".into(),
            self.thread_id.clone(),
            "--no-alt-screen".into(),
        ]);
        command
    }

    pub(crate) fn actor_tui_command(&self) -> Vec<String> {
        self.tui_command()
    }

    pub(crate) fn tui_environment(&self) -> BTreeMap<String, String> {
        self.environment.clone()
    }

    pub(crate) fn tui_ready(&self) -> bool {
        self.thread_materialized.load(AtomicOrdering::Acquire)
    }

    pub(crate) fn mark_thread_materialized(&self) {
        self.thread_materialized
            .store(true, AtomicOrdering::Release);
    }

    pub(crate) fn process_running(&self) -> bool {
        self.process
            .as_ref()
            .is_none_or(|process| process.running())
    }

    pub(crate) fn process_id(&self) -> Option<u32> {
        self.process.as_ref().and_then(|process| process.id())
    }

    pub(crate) async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> io::Result<Value> {
        self.protocol.request(method, params, timeout).await
    }

    pub(crate) async fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        self.protocol.respond_error(id, error).await
    }
}

pub(super) fn bind_workspace(root: &Path) -> io::Result<WorkspaceBinding> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Voice Analyst working directory must be a directory",
        ));
    }
    Ok(WorkspaceBinding { root })
}
