use super::*;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::sync::Arc;
use std::time::Duration;

impl AnalystSession {
    pub(super) async fn launch_prepared(
        binding: WorkspaceBinding,
        remote_tui_prefix: Vec<String>,
        command: Vec<String>,
        env: BTreeMap<String, String>,
        resume_thread_id: Option<String>,
        purpose: SessionPurpose,
    ) -> io::Result<Self> {
        Self::launch_prepared_with_cancel(
            binding,
            remote_tui_prefix,
            command,
            env,
            resume_thread_id,
            purpose,
            None,
        )
        .await
    }

    pub(super) async fn launch_prepared_with_cancel(
        binding: WorkspaceBinding,
        remote_tui_prefix: Vec<String>,
        command: Vec<String>,
        env: BTreeMap<String, String>,
        resume_thread_id: Option<String>,
        purpose: SessionPurpose,
        mut cancellation: Option<tokio::sync::watch::Receiver<bool>>,
    ) -> io::Result<Self> {
        let (process, lines) = lifecycle_timing::run_sync("codex.spawn", || {
            process::spawn_app_server(&command, &binding.root, &env)
        })?;
        let process = Arc::new(process);
        let work = async {
            let endpoint =
                lifecycle_timing::run("codex.endpoint", process::wait_for_endpoint(lines)).await?;
            let generation = uuid::Uuid::new_v4().simple().to_string();
            Self::connect(ConnectConfig {
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
            .await
        };
        tokio::pin!(work);
        let result = if let Some(cancel) = cancellation.as_mut() {
            if *cancel.borrow() {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "managed startup cancelled",
                ))
            } else {
                tokio::select! {
                    result = &mut work => result,
                    _ = cancel.wait_for(|value| *value) => Err(io::Error::new(io::ErrorKind::Interrupted, "managed startup cancelled")),
                }
            }
        } else {
            work.await
        };
        match result {
            Err(startup) => match process.stop() {
                Ok(()) => Err(startup),
                Err(cleanup) => Err(io::Error::other(StartupCleanupFailure {
                    owner: PendingManagedStartup(Arc::clone(&process)),
                    startup,
                    cleanup,
                })),
            },
            result => result,
        }
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
        let socket =
            lifecycle_timing::run("codex.connect", protocol::connect_with_retry(&endpoint)).await?;
        let protocol = ProtocolClient::new(
            socket,
            generation.clone(),
            process.as_ref().map(Arc::downgrade),
        );
        protocol
            .request(
                "initialize",
                json!({
                    "clientInfo":{"name":match purpose {
                        SessionPurpose::VoiceAnalyst => "cccc-voice-analyst",
                        SessionPurpose::Actor => "cccc-actor",
                        SessionPurpose::VoiceSecretary => "cccc-voice-secretary",
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
                params["developerInstructions"] = json!(super::launch::ANALYST_INSTRUCTIONS);
            }
            SessionPurpose::Actor => {
                params["personality"] = json!("pragmatic");
            }
            SessionPurpose::VoiceSecretary => {
                params
                    .as_object_mut()
                    .expect("thread params")
                    .remove("sandbox");
                params["permissions"] = super::launch_secretary::profile_params(&environment)?;
                params["developerInstructions"] = json!(super::launch_secretary::INSTRUCTIONS);
                params["baseInstructions"] = json!(
                    "You are a task-bound document and research agent. Follow the host developer instructions and use only the tools available in this task."
                );
                // Config maps merge with the user's config. Replacing mcp_servers
                // alone does not remove inherited servers, so disable every
                // server except this task's uniquely named host endpoint.
                let effective = protocol
                    .request(
                        "config/read",
                        json!({"includeLayers":false}),
                        Duration::from_secs(10),
                    )
                    .await?;
                for feature in [
                    "apps",
                    "plugins",
                    "browser_use",
                    "browser_use_external",
                    "hooks",
                    "multi_agent",
                ] {
                    if effective["config"]["features"][feature] == true {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "Codex host policy did not disable non-secretary capabilities",
                        ));
                    }
                }
                let own = environment
                    .get(super::launch_secretary::PROFILE_ENV)
                    .expect("validated permission identity");
                let servers = effective["config"]["mcp_servers"]
                    .as_object()
                    .ok_or_else(|| {
                        io::Error::other("Codex did not expose its effective MCP configuration")
                    })?;
                if !servers.contains_key(own) {
                    return Err(io::Error::other("Task MCP endpoint is missing"));
                }
                let mut overrides = serde_json::Map::new();
                for name in servers.keys().filter(|name| *name != own) {
                    overrides.insert(format!("mcp_servers.{name}.enabled"), json!(false));
                }
                let skills = protocol
                    .request(
                        "skills/list",
                        json!({"cwds":[binding.root]}),
                        Duration::from_secs(10),
                    )
                    .await?;
                let mut disabled = Vec::new();
                for entry in skills["data"]
                    .as_array()
                    .ok_or_else(|| io::Error::other("Codex did not expose its effective skills"))?
                {
                    for skill in entry["skills"]
                        .as_array()
                        .ok_or_else(|| io::Error::other("Codex skills response is incomplete"))?
                    {
                        let path = skill["path"]
                            .as_str()
                            .ok_or_else(|| io::Error::other("Codex skill path is missing"))?;
                        disabled.push(json!({"path":path,"enabled":false}));
                    }
                }
                overrides.insert("skills.config".into(), json!(disabled));
                params["config"] = json!(overrides);
            }
        }
        let requested_thread_id = resume_thread_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (started, thread_resumed) = if let Some(thread_id) = requested_thread_id {
            let mut resume_params = params.clone();
            resume_params["threadId"] = json!(thread_id);
            // Only the thread id is used. Full history arrives as one websocket
            // frame that outgrows its size limit on long threads, and the
            // disconnect then repeats on every reconnect.
            resume_params["excludeTurns"] = json!(true);
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
        if purpose == SessionPurpose::VoiceSecretary
            && (started["activePermissionProfile"]["id"].as_str()
                != environment
                    .get(super::launch_secretary::PROFILE_ENV)
                    .map(String::as_str)
                || started["sandbox"]["type"] != "workspaceWrite"
                || started["sandbox"]["networkAccess"] != false
                || started["runtimeWorkspaceRoots"] != json!([binding.root]))
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This Codex version did not confirm the secretary permission profile. Update Codex before using the global secretary.",
            ));
        }
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
        if !thread_resumed {
            // thread/start reserves an id/path but does not persist an empty
            // rollout. Native TUI resume needs that rollout, even on the same
            // app-server. Naming the new thread materializes it without a model
            // turn or synthetic conversation item; never rename resumed history.
            let name = match purpose {
                SessionPurpose::Actor => "CCCC Actor",
                SessionPurpose::VoiceAnalyst => "CCCC Voice Analyst",
                SessionPurpose::VoiceSecretary => "CCCC Voice Secretary",
            };
            protocol
                .request(
                    "thread/name/set",
                    json!({"threadId":thread_id,"name":name}),
                    Duration::from_secs(20),
                )
                .await?;
            let persisted = protocol
                .request(
                    "thread/read",
                    json!({"threadId":thread_id,"includeTurns":true}),
                    Duration::from_secs(20),
                )
                .await?;
            if persisted["thread"]["id"].as_str() != Some(thread_id.as_str())
                || !persisted["thread"]["turns"].is_array()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Codex did not expose the new thread's durable history for terminal resume",
                ));
            }
        }
        Ok(Self {
            #[cfg(test)]
            binding,
            generation,
            endpoint,
            thread_id,
            remote_tui_prefix,
            environment,
            protocol: ManagedProtocol::Codex(protocol),
            process,
            auxiliary_processes: Vec::new(),
            native_tui_command: None,
            cleanup_paths: Vec::new(),
            runtime: cccc_contracts::ActorRuntime::Codex,
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
}
