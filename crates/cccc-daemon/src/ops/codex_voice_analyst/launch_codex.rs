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
        let (process, lines) = lifecycle_timing::run_sync("codex.spawn", || {
            process::spawn_app_server(&command, &binding.root, &env)
        })?;
        let process = Arc::new(process);
        let endpoint =
            lifecycle_timing::run("codex.endpoint", process::wait_for_endpoint(lines)).await?;
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
        let socket =
            lifecycle_timing::run("codex.connect", protocol::connect_with_retry(&endpoint)).await?;
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
                params["developerInstructions"] = json!(super::launch::ANALYST_INSTRUCTIONS);
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
        if !thread_resumed {
            // thread/start reserves an id/path but does not persist an empty
            // rollout. Native TUI resume needs that rollout, even on the same
            // app-server. Naming the new thread materializes it without a model
            // turn or synthetic conversation item; never rename resumed history.
            let name = match purpose {
                SessionPurpose::Actor => "CCCC Actor",
                SessionPurpose::VoiceAnalyst => "CCCC Voice Analyst",
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
