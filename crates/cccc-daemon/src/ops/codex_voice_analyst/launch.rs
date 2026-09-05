use super::*;
use std::collections::BTreeMap;
use std::path::Path;

pub(super) const ANALYST_INSTRUCTIONS: &str = r#"You are the Voice Analyst behind CCCC Realtime Voice. In delegated speech, references such as 'the analyst', 'ask the analyst', or 'have the analyst check' refer to you: perform that investigation directly with your own tools. Never use runtime collaboration or sub-agent tools in this role. When additional execution is genuinely needed, coordinate an existing CCCC Group Foreman or peer through CCCC tools instead of creating an untracked second analyst. Investigate material claims with tools before answering.

The host starts you in a neutral CCCC-owned working directory. It is not a Working Group, repository scope, or implicit target. Every CCCC operation concerning a Group, Actor, task, message, ledger, or repository must use an explicit group_id and any required target identity. When the user asks about all Groups or names another Group, use CCCC tools to list or resolve live state. Never infer live state from CCCC_HOME directories or describe one Group snapshot as global state. Before repository investigation, resolve the intended Group and attached root, read the applicable repository instructions, and operate only on that explicit target. Delegate repository modification or durable work to the existing Group Foreman or peer instead of treating this neutral cwd as the project.

Use existing CCCC tools when live Group facts or durable Actor work are needed; hand off only when the requested outcome genuinely requires durable execution rather than your own investigation. Never claim that work was accepted unless the tool returned durable task or message facts. Keep progress substantive and the final concise and evidence-backed for speech; detailed work remains visible in the Analyst terminal."#;

impl AnalystSession {
    pub(crate) async fn launch(home: &HomeLayout, config: LaunchConfig) -> io::Result<Self> {
        let binding = bind_workspace(&config.workdir)?;
        cccc_core::codex_voice_settings::validate_private_environment(&config.environment)?;
        match config.runtime {
            cccc_contracts::ActorRuntime::Codex => {
                let mut env = config.environment;
                let prepared = super::launch_command::prepare(&config.command, &env)?;
                let mut command = prepared.app_server;
                if !super::super::codex_mcp::configure_global_user_mcp(home, &mut command, &mut env)
                {
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
            cccc_contracts::ActorRuntime::Claude => {
                Self::launch_claude(
                    home,
                    binding,
                    config.command,
                    config.environment,
                    config.resume_thread_id,
                    SessionPurpose::VoiceAnalyst,
                    None,
                )
                .await
            }
            cccc_contracts::ActorRuntime::Grok => {
                Self::launch_grok(
                    home,
                    binding,
                    config.command,
                    config.environment,
                    config.resume_thread_id,
                    SessionPurpose::VoiceAnalyst,
                    None,
                )
                .await
            }
            cccc_contracts::ActorRuntime::Opencode | cccc_contracts::ActorRuntime::Kilo => {
                Self::launch_opencode(
                    config.runtime,
                    home,
                    binding,
                    config.command,
                    config.environment,
                    config.resume_thread_id,
                    SessionPurpose::VoiceAnalyst,
                    None,
                )
                .await
            }
            runtime => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("Voice Analyst has no managed-session adapter for {runtime:?}"),
            )),
        }
    }

    pub(crate) async fn launch_actor(
        home: &HomeLayout,
        config: ActorLaunchConfig,
    ) -> io::Result<Self> {
        let binding = bind_workspace(&config.workdir)?;
        if config.runtime == cccc_contracts::ActorRuntime::Claude {
            return Self::launch_claude(
                home,
                binding,
                config.command,
                config.environment,
                None,
                SessionPurpose::Actor,
                Some((&config.group_id, &config.actor_id)),
            )
            .await;
        }
        if config.runtime == cccc_contracts::ActorRuntime::Grok {
            return Self::launch_grok(
                home,
                binding,
                config.command,
                config.environment,
                None,
                SessionPurpose::Actor,
                Some((&config.group_id, &config.actor_id)),
            )
            .await;
        }
        if matches!(
            config.runtime,
            cccc_contracts::ActorRuntime::Opencode | cccc_contracts::ActorRuntime::Kilo
        ) {
            return Self::launch_opencode(
                config.runtime,
                home,
                binding,
                config.command,
                config.environment,
                None,
                SessionPurpose::Actor,
                Some((&config.group_id, &config.actor_id)),
            )
            .await;
        }
        if config.runtime != cccc_contracts::ActorRuntime::Codex {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "Actor has no managed-session adapter for {:?}",
                    config.runtime
                ),
            ));
        }
        let mut env = config.environment;
        let prepared = super::launch_command::prepare(&config.command, &env)?;
        let session_command = prepared.app_server.clone();
        let identity_environment = env.clone();
        let resume_thread_id = lifecycle_timing::run_sync("codex.resume_lookup", || {
            super::super::runtime_session::prepare_codex_app_thread(
                home,
                &config.group_id,
                &config.actor_id,
                &binding.root,
                &session_command,
                &identity_environment,
                &prepared.model,
            )
        })?;
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
        if let Err(error) = lifecycle_timing::run_sync("codex.resume_record", || {
            super::super::runtime_session::record_codex_app_thread(
                home,
                &config.group_id,
                &config.actor_id,
                &config.workdir,
                &session_command,
                &identity_environment,
                super::super::runtime_session::CodexAppThread {
                    id: session.thread_id(),
                    resumed: session.thread_resumed,
                },
            )
        }) {
            tracing::warn!(
                %error,
                group_id = %config.group_id,
                actor_id = %config.actor_id,
                "failed to persist Codex Actor app-server thread"
            );
        }
        Ok(session)
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
        self.publish_event_with_delegation_for_test(message, None);
    }

    #[cfg(test)]
    pub(crate) fn publish_event_with_delegation_for_test(
        &self,
        message: Value,
        requested_delegation_id: Option<String>,
    ) {
        self.protocol.publish_for_test(AnalystEvent {
            generation: self.generation.clone(),
            message,
            requested_delegation_id,
        });
    }

    pub(crate) fn tui_command(&self) -> Vec<String> {
        if let Some(command) = &self.native_tui_command {
            return command.clone();
        }
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
        true
    }

    pub(crate) fn process_running(&self) -> bool {
        self.protocol.running()
            && self
                .process
                .as_ref()
                .is_none_or(|process| process.running())
            && self
                .auxiliary_processes
                .iter()
                .all(|process| process.running())
    }

    pub(crate) fn process_id(&self) -> Option<u32> {
        self.process.as_ref().and_then(|process| process.id())
    }

    pub(crate) async fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        self.protocol.respond_error(id, error).await
    }

    pub(crate) fn supports_steer(&self) -> bool {
        self.runtime == cccc_contracts::ActorRuntime::Codex
    }
}

pub(super) fn bind_workspace(root: &Path) -> io::Result<WorkspaceBinding> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "managed Agent working directory must be a directory",
        ));
    }
    Ok(WorkspaceBinding { root })
}
