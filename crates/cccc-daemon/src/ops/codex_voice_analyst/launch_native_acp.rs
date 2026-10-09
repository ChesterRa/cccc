use super::*;
use serde_json::json;

impl AnalystSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn launch_native_acp(
        home: &HomeLayout,
        binding: WorkspaceBinding,
        runtime: ActorRuntime,
        command: Vec<String>,
        mut env: BTreeMap<String, String>,
        resume: Option<String>,
        purpose: SessionPurpose,
        actor: Option<(&str, &str)>,
    ) -> io::Result<Self> {
        let options = native_acp::options(runtime, &command)?;
        let identity_command = if command.is_empty() {
            cccc_runtime::default_command(runtime)
        } else {
            command.clone()
        };
        let identity = super::super::runtime_session::native_acp::identity(
            &binding.root,
            &identity_command,
            &env,
        );
        let cccc = super::super::codex_mcp::configure_actor_cli(&mut env);
        #[cfg(test)]
        let cccc = cccc.or_else(|| env.remove("ACP_FIXTURE_CLI").map(PathBuf::from));
        let cccc = cccc.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "CCCC executable is unavailable for ACP MCP binding",
            )
        })?;
        let (group, actor_id, profile) =
            actor.map_or(("", "user", Some("full")), |(g, a)| (g, a, None));
        let mut mcp = acp_mcp_server(
            home,
            &cccc,
            group,
            actor_id,
            task_tool_profile(purpose, profile),
        );
        add_voice_mcp_origin(&mut mcp, &env, purpose);
        let receipt_path = actor
            .map(|(g, a)| {
                super::super::runtime_session::native_acp::receipt_path(home, g, a, runtime)
            })
            .transpose()?;
        let resume = if let Some(path) = &receipt_path {
            super::super::runtime_session::native_acp::prepare(path, &identity)?
        } else {
            resume
        };
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let mut launch_command = options.command.clone();
        if runtime == ActorRuntime::Copilot {
            if purpose == SessionPurpose::VoiceSecretary {
                let executable = cccc_runtime::resolve_command_executable(&launch_command, &env);
                let mut catalog = tokio::process::Command::new(&executable[0]);
                catalog
                    .args(["--no-auto-update", "mcp", "list", "--json"])
                    .current_dir(&binding.root)
                    .envs(&env)
                    .kill_on_drop(true);
                let output =
                    tokio::time::timeout(std::time::Duration::from_secs(10), catalog.output())
                        .await
                        .map_err(|_| {
                            io::Error::new(
                                io::ErrorKind::TimedOut,
                                "Copilot MCP catalog inspection timed out",
                            )
                        })??;
                if !output.status.success() {
                    return Err(io::Error::other(
                        "Copilot MCP catalog inspection failed; no secretary prompt was sent",
                    ));
                }
                let catalog: Value = serde_json::from_slice(&output.stdout)
                    .map_err(|_| io::Error::other("Copilot returned an invalid MCP catalog"))?;
                restrict_secretary_copilot_mcp(&mut launch_command, &catalog)?;
            }
            // Process-local stdio MCP injection: the official ACP session
            // mcpServers surface currently accepts remote servers only.
            let env = mcp["env"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    Some((entry["name"].as_str()?.to_owned(), entry["value"].clone()))
                })
                .collect::<serde_json::Map<String, Value>>();
            let config = json!({"mcpServers":{"cccc":{"type":"local","command":mcp["command"],"args":mcp["args"],"env":env,"tools":["*"]}}});
            // This configuration contains only the CCCC-scoped transport
            // identity, no provider credentials and no global config writes.
            launch_command.extend([
                "--additional-mcp-config".into(),
                serde_json::to_string(&config).map_err(io::Error::other)?,
            ]);
        }
        // Resolve npm .cmd/.bat shims using the same PATH/PATHEXT rules as
        // runtime detection. std::process::Command handles batch-file quoting;
        // the portable-pty cmd wrapper is not appropriate for this stdio path.
        let launch_command = cccc_runtime::resolve_command_executable(&launch_command, &env);
        let (process, stdin, stdout) =
            process::spawn_piped_quiet(&launch_command, &binding.root, &env)?;
        let protocol = AcpClient::new(
            stdin,
            stdout,
            generation.clone(),
            native_acp::name(runtime),
            if options.unrestricted {
                acp::PermissionPolicy::AllowOnce
            } else {
                acp::PermissionPolicy::Interactive
            },
            acp::PromptCompletion::ResponseWithActivityReceipt,
        )?;
        let initialized = native_acp::initialize(
            &protocol,
            runtime,
            &binding.root,
            resume.as_deref(),
            mcp,
            &options,
        )
        .await;
        let (id, resumed) = match initialized {
            Ok(result) => result,
            Err(error) => {
                protocol.close().await;
                process.stop()?;
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "{} ACP initialization failed; check native CLI login and launch options. Attempted sessions are never replaced automatically: {error}",
                        native_acp::name(runtime)
                    ),
                ));
            }
        };
        if purpose != SessionPurpose::VoiceSecretary {
            let receipt = if let Some(path) = receipt_path {
                if let Err(error) = super::super::runtime_session::native_acp::record(
                    &path, &identity, &id, resumed,
                ) {
                    protocol.close().await;
                    process.stop()?;
                    return Err(error);
                }
                super::super::runtime_session::native_acp::PromptReceipt::actor(
                    path,
                    id.clone(),
                    resumed,
                )
            } else {
                super::super::runtime_session::native_acp::PromptReceipt::voice(
                    home,
                    id.clone(),
                    resumed,
                )
            };
            protocol.set_prompt_receipt(receipt);
        }
        Ok(Self {
            #[cfg(test)]
            binding,
            generation,
            runtime,
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

fn restrict_secretary_copilot_mcp(command: &mut Vec<String>, catalog: &Value) -> io::Result<()> {
    let servers = catalog["mcpServers"]
        .as_object()
        .ok_or_else(|| io::Error::other("Copilot returned an unsupported MCP catalog"))?;
    for name in servers.keys().filter(|name| name.as_str() != "cccc") {
        command.extend(["--disable-mcp-server".into(), name.clone()]);
    }
    if !command.iter().any(|arg| arg == "--disable-builtin-mcps") {
        command.push("--disable-builtin-mcps".into());
    }
    command.extend(["--enable-mcp-server".into(), "cccc".into()]);
    if !command.iter().any(|arg| arg == "--no-custom-instructions") {
        command.push("--no-custom-instructions".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secretary_disables_inherited_copilot_servers_without_changing_yolo() {
        let mut command = vec!["copilot".into(), "--allow-all".into(), "--acp".into()];
        restrict_secretary_copilot_mcp(
            &mut command,
            &json!({"mcpServers":{"cccc":{},"user-extra":{},"plugin-extra":{}}}),
        )
        .expect("restrict catalog");
        assert!(command.iter().any(|arg| arg == "--allow-all"));
        assert!(
            command
                .windows(2)
                .any(|args| args == ["--disable-mcp-server", "user-extra"])
        );
        assert!(
            command
                .windows(2)
                .any(|args| args == ["--disable-mcp-server", "plugin-extra"])
        );
        assert!(
            !command
                .windows(2)
                .any(|args| args == ["--disable-mcp-server", "cccc"])
        );
        assert!(restrict_secretary_copilot_mcp(&mut command, &json!({})).is_err());
    }
}
