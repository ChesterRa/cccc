use super::*;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::sync::atomic::AtomicBool;

impl AnalystSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn launch_opencode(
        home: &HomeLayout,
        binding: WorkspaceBinding,
        command: Vec<String>,
        mut env: BTreeMap<String, String>,
        requested_session_id: Option<String>,
        purpose: SessionPurpose,
        actor: Option<(&str, &str, cccc_contracts::RunnerKind)>,
    ) -> io::Result<Self> {
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let cccc = super::super::codex_mcp::configure_actor_cli(&mut env).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "CCCC executable is unavailable for OpenCode MCP binding",
            )
        })?;
        env.insert(
            "CCCC_HOME".into(),
            home.root().to_string_lossy().into_owned(),
        );
        let (group_id, actor_id, tool_profile) = actor
            .map_or(("", "user", Some("full")), |(group_id, actor_id, _)| {
                (group_id, actor_id, None)
            });
        env.insert("CCCC_GROUP_ID".into(), group_id.into());
        env.insert("CCCC_ACTOR_ID".into(), actor_id.into());
        if let Some(profile) = tool_profile {
            env.insert("CCCC_MCP_TOOL_PROFILE".into(), profile.into());
        }
        let mcp_server = acp_mcp_server(home, &cccc, group_id, actor_id, tool_profile);
        let session_command = command.clone();
        let prepared = opencode::prepare(&command, &env)?;
        let resume_session_id = if let Some((group_id, actor_id, _)) = actor {
            super::super::runtime_session::prepare_opencode_managed_session(
                home,
                group_id,
                actor_id,
                &binding.root,
                &session_command,
                &env,
            )?
        } else {
            requested_session_id
        };
        let launched = opencode::launch(
            prepared,
            &binding.root,
            env,
            &generation,
            purpose,
            resume_session_id.as_deref(),
            mcp_server,
        )
        .await?;
        if let Some((group_id, actor_id, runner)) = actor
            && let Err(error) = super::super::runtime_session::record_opencode_managed_session(
                home,
                group_id,
                actor_id,
                &binding.root,
                &session_command,
                &launched.environment,
                &launched.session_id,
                launched.resumed,
                runner,
            )
        {
            tracing::warn!(%error, %group_id, %actor_id, "failed to persist OpenCode managed session");
        }
        Ok(Self {
            #[cfg(test)]
            binding,
            generation,
            runtime: cccc_contracts::ActorRuntime::Opencode,
            endpoint: String::new(),
            thread_id: launched.session_id,
            remote_tui_prefix: Vec::new(),
            environment: launched.environment,
            protocol: ManagedProtocol::Acp(launched.protocol),
            process: Some(launched.process),
            auxiliary_processes: Vec::new(),
            native_tui_command: Some(launched.tui_command),
            cleanup_paths: Vec::new(),
            thread_materialized: AtomicBool::new(true),
            thread_resumed: launched.resumed,
            delegations: tokio::sync::Mutex::new(HashMap::new()),
        })
    }
}
