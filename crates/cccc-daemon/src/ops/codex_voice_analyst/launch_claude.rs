use super::*;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::io;

impl AnalystSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn launch_claude(
        home: &HomeLayout,
        binding: WorkspaceBinding,
        command: Vec<String>,
        mut environment: BTreeMap<String, String>,
        requested_session_id: Option<String>,
        purpose: SessionPurpose,
        actor: Option<(&str, &str)>,
    ) -> io::Result<Self> {
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let cccc =
            super::super::codex_mcp::configure_actor_cli(&mut environment).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "CCCC executable is unavailable for Claude MCP binding",
                )
            })?;
        environment.insert(
            "CCCC_HOME".into(),
            home.root().to_string_lossy().into_owned(),
        );
        let (group_id, actor_id, tool_profile, settings_key) = actor.map_or_else(
            || ("", "user", Some("full"), "voice-analyst".to_owned()),
            |(group_id, actor_id)| {
                (
                    group_id,
                    actor_id,
                    None,
                    format!("actor:{group_id}:{actor_id}"),
                )
            },
        );
        let tool_profile = task_tool_profile(purpose, tool_profile);
        let settings_key = if purpose == SessionPurpose::VoiceSecretary {
            format!("voice-secretary:{generation}")
        } else {
            settings_key
        };
        environment.insert("CCCC_GROUP_ID".into(), group_id.into());
        environment.insert("CCCC_ACTOR_ID".into(), actor_id.into());
        if let Some(tool_profile) = tool_profile {
            environment.insert("CCCC_MCP_TOOL_PROFILE".into(), tool_profile.into());
        }
        let mut mcp_environment = serde_json::Map::from_iter([
            (
                "CCCC_HOME".into(),
                json!(home.root().to_string_lossy().into_owned()),
            ),
            ("CCCC_GROUP_ID".into(), json!(group_id)),
            ("CCCC_ACTOR_ID".into(), json!(actor_id)),
        ]);
        if let Some(tool_profile) = tool_profile {
            mcp_environment.insert("CCCC_MCP_TOOL_PROFILE".into(), json!(tool_profile));
        }
        if purpose == SessionPurpose::VoiceSecretary
            && let Some(path) = environment.get("CCCC_SECRETARY_TASK_TOKEN_FILE")
        {
            mcp_environment.insert("CCCC_SECRETARY_TASK_TOKEN_FILE".into(), json!(path));
        }
        if actor.is_none()
            && let Some(origin) = environment.get(cccc_core::voice_notifications::ORIGIN_ENV)
        {
            mcp_environment.insert(
                cccc_core::voice_notifications::ORIGIN_ENV.into(),
                json!(origin),
            );
        }
        let mcp_server = json!({
            "command":cccc,
            "args":["mcp"],
            "env":mcp_environment,
        });
        let session_command = command.clone();
        let resume_attempt = if let Some((group_id, actor_id)) = actor {
            super::super::runtime_session::prepare_claude_managed_session(
                home,
                group_id,
                actor_id,
                &binding.root,
                &session_command,
                &environment,
            )?
        } else {
            None
        };
        let resume_session_id = resume_attempt
            .as_ref()
            .map(|attempt| attempt.session_id.as_str())
            .or(requested_session_id.as_deref());
        let result = match claude::prepare(
            home,
            &command,
            &environment,
            &binding.root,
            &settings_key,
            purpose,
            mcp_server,
        ) {
            Ok(prepared) => {
                // Transcript validation holds a large future. Keep it off the
                // managed worker's stack, including unoptimized native builds.
                Box::pin(claude::launch(
                    prepared,
                    &binding.root,
                    &generation,
                    purpose,
                    resume_session_id,
                ))
                .await
            }
            Err(error) => Err(error),
        };
        let mut launched = match result {
            Ok(launched) => launched,
            Err(error) => {
                if purpose == SessionPurpose::VoiceSecretary
                    && claude::cleanup_secretary_owner(home, &binding.root)
                        .await
                        .is_ok()
                {
                    claude::remove_secretary_settings(home, &settings_key)?;
                }
                if let Some((group_id, actor_id)) = actor
                    && let Some(attempt) = resume_attempt.as_ref()
                    && let Some((diagnostic, blocked)) = claude::resume_diagnostic(&error)
                {
                    super::super::runtime_session::record_claude_resume_failure(
                        home, group_id, actor_id, attempt, diagnostic, blocked,
                    )?;
                    return Err(if blocked {
                        io::Error::other(super::super::runtime_session::ClaudeResumeBlocked(
                            diagnostic.into(),
                        ))
                    } else {
                        io::Error::new(error.kind(), diagnostic)
                    });
                }
                return Err(error);
            }
        };
        if purpose == SessionPurpose::VoiceSecretary {
            launched
                .cleanup_paths
                .push(claude::settings_path(home, &settings_key));
            launched
                .cleanup_paths
                .push(claude::secretary_owner_path(&binding.root)?);
        }
        if let Some((group_id, actor_id)) = actor
            && let Err(error) = super::super::runtime_session::record_claude_managed_session(
                home,
                group_id,
                actor_id,
                &binding.root,
                &session_command,
                &environment,
                &launched.session_id,
                launched.resumed,
            )
        {
            tracing::warn!(%error, %group_id, %actor_id, "failed to persist Claude managed session");
        }
        Ok(Self {
            #[cfg(test)]
            binding,
            generation,
            runtime: cccc_contracts::ActorRuntime::Claude,
            endpoint: String::new(),
            thread_id: launched.session_id,
            remote_tui_prefix: Vec::new(),
            environment: launched.environment,
            protocol: ManagedProtocol::Claude(launched.protocol),
            process: None,
            auxiliary_processes: Vec::new(),
            native_tui_command: Some(launched.tui_command),
            cleanup_paths: launched.cleanup_paths,
            thread_resumed: launched.resumed,
            delegations: tokio::sync::Mutex::new(HashMap::new()),
        })
    }
}
