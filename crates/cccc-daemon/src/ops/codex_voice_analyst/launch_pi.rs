use super::*;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

impl AnalystSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn launch_pi(
        home: &HomeLayout,
        binding: WorkspaceBinding,
        command: Vec<String>,
        mut environment: BTreeMap<String, String>,
        resume: Option<String>,
        purpose: SessionPurpose,
        actor: Option<(&str, &str)>,
    ) -> io::Result<Self> {
        let identity_command = if command.is_empty() {
            vec!["pi".into()]
        } else {
            command
        };
        let mut command = prepare_command(&identity_command)?;
        if purpose == SessionPurpose::VoiceSecretary
            && command.iter().any(|arg| arg == "--exclude-tools")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Secretary tool authority is owned by CCCC; remove --exclude-tools from the Pi command",
            ));
        }
        let identity = super::super::runtime_session::pi::identity(
            &binding.root,
            &identity_command,
            &environment,
        );
        let cccc = super::super::codex_mcp::configure_actor_cli(&mut environment);
        #[cfg(test)]
        let cccc = cccc.or_else(|| environment.remove("PI_FIXTURE_CLI").map(PathBuf::from));
        let cccc = cccc.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "CCCC executable is unavailable for Pi MCP binding",
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
        add_voice_mcp_origin(&mut mcp, &environment, purpose);
        let env = mcp["env"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| Some((item["name"].as_str()?.to_owned(), item["value"].clone())))
            .collect::<serde_json::Map<String, Value>>();
        let server =
            json!({"command":mcp["command"],"args":mcp["args"],"env":env,"exposure":"direct"});
        validate_mcp_configuration(&binding.root, &environment, &server)?;
        environment.insert("PI_SKIP_VERSION_CHECK".into(), "1".into());
        environment.insert("PI_TELEMETRY".into(), "0".into());
        command = cccc_runtime::resolve_command_executable(&command, &environment);
        let mut version = std::process::Command::new(&command[0]);
        version
            .arg("--version")
            .current_dir(&binding.root)
            .envs(&environment);
        let output =
            cccc_runtime::capture_command(&mut version, None, Duration::from_secs(10), 4096)
                .await?;
        if !output.status.success() || output.stdout_truncated || !supported_version(&output.stdout)
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Pi requires official @earendil-works/pi-coding-agent 1.1.0 or newer; the legacy @mariozechner package is unsupported",
            ));
        }
        let sessions = if let Some((g, a)) = actor {
            super::super::runtime_session::pi::receipt_path(home, g, a)?
                .parent()
                .expect("receipt directory")
                .join("pi")
                .join(a)
                .join("sessions")
        } else if purpose == SessionPurpose::VoiceSecretary {
            binding.root.join("pi-sessions")
        } else {
            home.daemon_dir().join("pi-analyst").join("sessions")
        };
        std::fs::create_dir_all(&sessions)?;
        let sessions = std::fs::canonicalize(sessions)?;
        let receipt_path = actor
            .map(|(g, a)| super::super::runtime_session::pi::receipt_path(home, g, a))
            .transpose()?;
        let restored = if let Some(path) = &receipt_path {
            super::super::runtime_session::pi::prepare(path, &identity, &sessions)?
        } else if let Some(id) = resume {
            Some(super::super::runtime_session::pi::Resume {
                file: Some(super::super::runtime_session::pi::locate(&sessions, &id)?),
                id,
                attempted: true,
            })
        } else {
            None
        };
        let resumed = restored.as_ref().is_some_and(|session| session.attempted);
        let id = restored.as_ref().map_or_else(
            || uuid::Uuid::new_v4().to_string(),
            |session| session.id.clone(),
        );
        command.extend([
            "--mode".into(),
            "rpc".into(),
            "--session-dir".into(),
            sessions.to_string_lossy().into_owned(),
        ]);
        if let Some(file) = restored.and_then(|session| session.file) {
            command.extend(["--session".into(), file.to_string_lossy().into_owned()]);
        } else {
            command.extend(["--session-id".into(), id.clone()]);
        }
        // Explicit extensions are session-only. Discovered extensions cannot introduce
        // an unowned RPC turn or replace CCCC task authority during startup.
        command.extend(["--no-extensions".into(), "-e".into(), "builtin:mcp".into()]);
        if purpose == SessionPurpose::VoiceSecretary {
            command.extend([
                "--no-context-files".into(),
                "--no-skills".into(),
                "--no-prompt-templates".into(),
                "--tools".into(),
                "read,bash,edit,write,grep,find,ls,mcp__cccc__*".into(),
            ]);
        }
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let owned = home.daemon_dir().join("pi-managed").join(&generation);
        std::fs::create_dir_all(&owned)?;
        let extension = owned.join("cccc.mjs");
        let mut cleanup_paths = vec![extension.clone()];
        let prepared = (|| -> io::Result<()> {
            write_private(
                &extension,
                format!(
                    "export default function(pi) {{ pi.registerMcpServer(\"cccc\", {}); }}\n",
                    serde_json::to_string(&server).map_err(io::Error::other)?
                )
                .as_bytes(),
            )?;
            command.extend(["-e".into(), extension.to_string_lossy().into_owned()]);
            let instructions = match purpose {
                SessionPurpose::VoiceSecretary => Some(launch_secretary::INSTRUCTIONS),
                SessionPurpose::VoiceAnalyst => Some(launch::ANALYST_INSTRUCTIONS),
                SessionPurpose::Actor => None,
            };
            if let Some(instructions) = instructions {
                let file = owned.join("instructions.md");
                write_private(&file, instructions.as_bytes())?;
                cleanup_paths.push(file.clone());
                command.extend([
                    "--append-system-prompt".into(),
                    file.to_string_lossy().into_owned(),
                ]);
            }
            Ok(())
        })();
        if let Err(error) = prepared {
            let _ = std::fs::remove_dir_all(&owned);
            return Err(error);
        }
        let (owner, stdin, stdout) =
            match process::spawn_piped_quiet(&command, &binding.root, &environment) {
                Ok(value) => value,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&owned);
                    return Err(error);
                }
            };
        let owner = Arc::new(owner);
        let protocol = match PiRpcClient::new(stdin, stdout, generation.clone(), id.clone()) {
            Ok(protocol) => protocol,
            Err(error) => return startup_failure(&owner, &owned, error),
        };
        let initialized = async {
            let state = protocol
                .request("get_state", json!({}), Duration::from_secs(15))
                .await?;
            if state["isStreaming"] != false
                || state["isCompacting"] != false
                || state["pendingMessageCount"] != 0
            {
                return Err(io::Error::other(
                    "Pi did not initialize as an idle controlled session",
                ));
            }
            let file = state["sessionFile"]
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| io::Error::other("Pi omitted its persistent session path"))?;
            super::super::runtime_session::pi::validate_path(&file, &sessions)?;
            if purpose != SessionPurpose::VoiceSecretary {
                let receipt = if let Some(path) = receipt_path {
                    super::super::runtime_session::pi::record(
                        &path, &identity, &id, &file, resumed,
                    )?;
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
            Ok::<(), io::Error>(())
        }
        .await;
        if let Err(error) = initialized {
            protocol.close().await;
            return startup_failure(&owner, &owned, error);
        }
        Ok(Self {
            #[cfg(test)]
            binding,
            generation,
            runtime: ActorRuntime::Pi,
            endpoint: String::new(),
            thread_id: id,
            remote_tui_prefix: Vec::new(),
            environment,
            protocol: ManagedProtocol::Pi(protocol),
            process: Some(owner),
            auxiliary_processes: Vec::new(),
            native_tui_command: None,
            cleanup_paths: {
                cleanup_paths.push(owned);
                cleanup_paths
            },
            thread_resumed: resumed,
            delegations: tokio::sync::Mutex::new(HashMap::new()),
        })
    }
}

fn startup_failure(
    owner: &Arc<ChildOwner>,
    owned: &Path,
    startup: io::Error,
) -> io::Result<AnalystSession> {
    match owner.stop() {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(owned);
            Err(startup)
        }
        Err(cleanup) => Err(io::Error::other(StartupCleanupFailure {
            owner: PendingManagedStartup(Arc::clone(owner)),
            startup,
            cleanup,
        })),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn supported_version(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let text = text
        .trim()
        .strip_prefix("pi ")
        .unwrap_or(text.trim())
        .trim_start_matches('v');
    if text.contains('-') {
        return false;
    }
    let parts = text
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>();
    parts.is_ok_and(|parts| parts.len() == 3 && (parts[0], parts[1], parts[2]) >= (1, 1, 0))
}

fn prepare_command(configured: &[String]) -> io::Result<Vec<String>> {
    let Some(program) = configured.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Pi command is empty",
        ));
    };
    let name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let valid = name == "pi"
        || cfg!(windows)
            && matches!(
                name.to_ascii_lowercase().as_str(),
                "pi.exe" | "pi.cmd" | "pi.bat"
            );
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Managed Pi requires the native pi executable, without a wrapper",
        ));
    }
    let mut normalized = vec![program.clone()];
    let mut index = 1;
    while index < configured.len() {
        let arg = &configured[index];
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
        if matches!(
            flag,
            "--provider"
                | "--model"
                | "--thinking"
                | "--models"
                | "--tools"
                | "--exclude-tools"
                | "--system-prompt"
                | "--append-system-prompt"
                | "-t"
        ) {
            if let Some(value) = inline {
                if value.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Pi option is missing its value",
                    ));
                }
            } else {
                index += 1;
                if configured
                    .get(index)
                    .is_none_or(|value| value.is_empty() || value.starts_with('-'))
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Pi option is missing its value",
                    ));
                }
            }
        } else if !matches!(
            flag,
            "--offline"
                | "--no-approve"
                | "--approve"
                | "--no-context-files"
                | "--no-skills"
                | "--no-prompt-templates"
                | "--no-themes"
        ) || inline.is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Pi profile contains an unsupported argument; session, RPC transport and extensions are owned by CCCC",
            ));
        }
        if let Some(value) = inline {
            normalized.extend([flag.to_owned(), value.to_owned()]);
        } else {
            normalized.push(arg.clone());
            if matches!(
                flag,
                "--provider"
                    | "--model"
                    | "--thinking"
                    | "--models"
                    | "--tools"
                    | "--exclude-tools"
                    | "--system-prompt"
                    | "--append-system-prompt"
                    | "-t"
            ) {
                normalized.push(configured[index].clone());
            }
        }
        index += 1;
    }
    Ok(normalized)
}

fn validate_mcp_configuration(
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    expected: &Value,
) -> io::Result<()> {
    let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = environment
        .get(home_key)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(home_key).map(PathBuf::from))
        .ok_or_else(|| {
            io::Error::other("Pi provider home is unavailable; set PI_CODING_AGENT_DIR explicitly")
        });
    let agent = if let Some(value) = environment
        .get("PI_CODING_AGENT_DIR")
        .cloned()
        .or_else(|| std::env::var("PI_CODING_AGENT_DIR").ok())
        .filter(|value| !value.is_empty())
    {
        if value == "~" {
            home?
        } else if let Some(rest) = value
            .strip_prefix("~/")
            .or_else(|| value.strip_prefix("~\\"))
        {
            home?.join(rest)
        } else {
            PathBuf::from(value)
        }
    } else {
        home?.join(".pi").join("agent")
    };
    let agent = if agent.is_absolute() {
        agent
    } else {
        cwd.join(agent)
    };
    for file in [agent.join("mcp.json"), cwd.join(".pi").join("mcp.json")] {
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(io::Error::other("Pi MCP configuration is unreadable")),
        };
        let config: Value = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::other("Pi MCP configuration is invalid"))?;
        if config["mcpServers"].as_object().is_some_and(|servers| {
            servers.keys().any(|name| {
                // Pi validates [A-Za-z0-9_-] names, replaces '-' with '_',
                // then joins server and tool with '__'. Trailing '_' and
                // nested '__' namespaces would pass the CCCC tool allowlist.
                name != "cccc"
                    && format!("mcp__{}__", name.replace('-', "_")).starts_with("mcp__cccc__")
            })
        }) {
            return Err(io::Error::other(
                "An existing Pi MCP server conflicts with CCCC's reserved tool namespace; rename that server before managed startup",
            ));
        }
        if let Some(server) = config["mcpServers"].get("cccc")
            && server != expected
        {
            return Err(io::Error::other(
                "An existing Pi cccc MCP entry overrides the managed task binding; remove that conflicting entry or make its command, scope and direct exposure match",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pi_native_version_and_command_admission_are_explicit() {
        assert!(supported_version(b"1.1.0\n"));
        assert!(supported_version(b"pi v1.2.3\n"));
        assert!(!supported_version(b"0.73.1"));
        assert!(!supported_version(b"1.0.99"));
        assert!(!supported_version(b"garbage 1.1.0"));
        assert!(
            prepare_command(&[
                "pi".into(),
                "--provider".into(),
                "fixture".into(),
                "--model=fixture".into()
            ])
            .is_ok()
        );
        for arg in [
            "--mode=rpc",
            "--session-id=x",
            "--api-key=secret",
            "--extension=x",
            "prompt",
        ] {
            assert!(prepare_command(&["pi".into(), arg.into()]).is_err());
        }
        assert!(prepare_command(&["node".into(), "pi.js".into()]).is_err());
    }
    #[test]
    fn pi_conflicting_mcp_entry_is_rejected_without_rewriting() {
        let tmp = tempfile::tempdir().expect("create isolated Pi MCP configuration home");
        let agent = tmp.path().join("agent");
        std::fs::create_dir(&agent).expect("create Pi agent configuration directory");
        let file = agent.join("mcp.json");
        let bytes =
            br#"{"mcpServers":{"cccc":{"command":"wrong"},"extra":{"command":"unchanged"}}}"#;
        std::fs::write(&file, bytes).expect("write conflicting MCP fixture");
        let env = BTreeMap::from([(
            "PI_CODING_AGENT_DIR".into(),
            agent.to_string_lossy().into_owned(),
        )]);
        assert!(
            validate_mcp_configuration(
                tmp.path(),
                &env,
                &json!({"command":"cccc","exposure":"direct"})
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(file).expect("read unchanged conflicting MCP fixture"),
            bytes
        );
    }
    #[test]
    fn pi_mcp_names_cannot_alias_the_owned_cccc_tool_prefix() {
        let tmp = tempfile::tempdir().expect("create isolated namespace fixture home");
        let agent = tmp.path().join("agent");
        std::fs::create_dir(&agent).expect("create namespace fixture configuration directory");
        let file = agent.join("mcp.json");
        let env = BTreeMap::from([(
            "PI_CODING_AGENT_DIR".into(),
            agent.to_string_lossy().into_owned(),
        )]);
        for name in ["cccc__extra", "cccc--extra", "cccc_", "cccc-"] {
            let bytes = serde_json::to_vec(
                &json!({"mcpServers":{name:{"command":"fixture","exposure":"direct"}}}),
            )
            .expect("encode reserved MCP namespace fixture");
            std::fs::write(&file, &bytes).expect("write reserved MCP namespace fixture");
            let error = validate_mcp_configuration(
                tmp.path(),
                &env,
                &json!({"command":"cccc","exposure":"direct"}),
            )
            .expect_err("reserved MCP namespace must be rejected");
            assert!(error.to_string().contains("reserved tool namespace"));
            assert_eq!(
                std::fs::read(&file).expect("read unchanged reserved MCP fixture"),
                bytes
            );
        }
        for name in ["extra", "cccc_extra", "other-cccc"] {
            std::fs::write(
                &file,
                serde_json::to_vec(
                    &json!({"mcpServers":{name:{"command":"fixture","exposure":"direct"}}}),
                )
                .expect("encode nonconflicting MCP namespace fixture"),
            )
            .expect("write nonconflicting MCP namespace fixture");
            assert!(
                validate_mcp_configuration(
                    tmp.path(),
                    &env,
                    &json!({"command":"cccc","exposure":"direct"})
                )
                .is_ok()
            );
        }
    }
}
