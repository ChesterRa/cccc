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
        let mcp_server = json!({
            "command":cccc,
            "args":["mcp"],
            "env":mcp_environment,
        });
        let session_command = command.clone();
        let resume_session_id = if let Some((group_id, actor_id)) = actor {
            super::super::runtime_session::prepare_claude_managed_session(
                home,
                group_id,
                actor_id,
                &binding.root,
                &session_command,
                &environment,
            )?
        } else {
            requested_session_id
        };
        let prepared = claude::prepare(
            home,
            &command,
            &environment,
            &binding.root,
            &settings_key,
            purpose,
            mcp_server,
        )?;
        let launched = claude::launch(
            prepared,
            &binding.root,
            &generation,
            purpose,
            resume_session_id.as_deref(),
        )
        .await?;
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
