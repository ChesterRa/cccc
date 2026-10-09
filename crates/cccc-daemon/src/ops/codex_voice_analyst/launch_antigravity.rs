use super::*;

pub(crate) async fn login_antigravity(home: &HomeLayout) -> io::Result<()> {
    let install_home = home.clone();
    let install =
        tokio::task::spawn_blocking(move || crate::antigravity_acp_setup::ensure(&install_home))
            .await
            .map_err(io::Error::other)??;
    let (process, stdin, stdout) = process::spawn_piped_for_login(
        &install.command,
        &install.provider_home,
        &install.environment,
    )?;
    let protocol = AcpClient::new(
        stdin,
        stdout,
        uuid::Uuid::new_v4().simple().to_string(),
        "antigravity",
        acp::PermissionPolicy::Reject,
        acp::PromptCompletion::Response,
    )?;
    let result=async {
        protocol.request("initialize",serde_json::json!({"protocolVersion":1,"clientInfo":{"name":"cccc-setup","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false}}),std::time::Duration::from_secs(40)).await?;
        protocol.request("authenticate",serde_json::json!({"methodId":"oauth-personal"}),std::time::Duration::from_secs(600)).await.map(|_|())
    }.await;
    protocol.close().await;
    process.stop()?;
    result
}

impl AnalystSession {
    pub(super) async fn launch_antigravity(
        home: &HomeLayout,
        binding: WorkspaceBinding,
        command: Vec<String>,
        mut env: BTreeMap<String, String>,
        resume: Option<String>,
        purpose: SessionPurpose,
        actor: Option<(&str, &str)>,
    ) -> io::Result<Self> {
        let options = antigravity::options(&command)?;
        let install_home = home.clone();
        let install = tokio::task::spawn_blocking(move || {
            crate::antigravity_acp_setup::ensure(&install_home)
        })
        .await
        .map_err(io::Error::other)??;
        env.extend(install.environment);
        let cccc = super::super::codex_mcp::configure_actor_cli(&mut env);
        #[cfg(test)]
        let cccc = cccc.or_else(|| env.remove("AGY_FIXTURE_CLI").map(std::path::PathBuf::from));
        let cccc = cccc.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "CCCC executable is unavailable for Antigravity MCP binding",
            )
        })?;
        let (group_id, actor_id, profile) = actor
            .map_or(("", "user", Some("full")), |(group, actor)| {
                (group, actor, None)
            });
        let mut mcp = acp_mcp_server(
            home,
            &cccc,
            group_id,
            actor_id,
            task_tool_profile(purpose, profile),
        );
        add_voice_mcp_origin(&mut mcp, &env, purpose);
        let resume = if let Some((group, actor)) = actor {
            super::super::runtime_session::antigravity::prepare(
                home,
                group,
                actor,
                &binding.root,
                &command,
                &env,
            )?
        } else {
            resume
        };
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let (process, stdin, stdout) =
            process::spawn_piped_quiet(&install.command, &binding.root, &env)?;
        let protocol = AcpClient::new(
            stdin,
            stdout,
            generation.clone(),
            "antigravity",
            antigravity::permission_policy(&options),
            acp::PromptCompletion::ResponseWithActivityReceipt,
        )?;
        let (id, resumed) = match antigravity::initialize(
            &protocol,
            &binding.root,
            resume.as_deref(),
            mcp,
            &options,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                protocol.close().await;
                process.stop()?;
                return Err(error);
            }
        };
        if let Some((group, actor)) = actor {
            // Resume persistence is required; a failed write rolls launch back.
            if let Err(error) = super::super::runtime_session::antigravity::record(
                home,
                group,
                actor,
                &binding.root,
                &command,
                &env,
                (&id, resumed),
            ) {
                protocol.close().await;
                process.stop()?;
                return Err(error);
            }
        }
        Ok(Self {
            #[cfg(test)]
            binding,
            generation,
            runtime: ActorRuntime::Antigravity,
            endpoint: String::new(),
            thread_id: id,
            remote_tui_prefix: Vec::new(),
            environment: env,
            protocol: ManagedProtocol::Acp(protocol),
            process: Some(Arc::new(process)),
            auxiliary_processes: Vec::new(),
            native_tui_command: None,
            cleanup_paths: Vec::new(),
            thread_resumed: resumed,
            delegations: tokio::sync::Mutex::new(HashMap::new()),
        })
    }
}
