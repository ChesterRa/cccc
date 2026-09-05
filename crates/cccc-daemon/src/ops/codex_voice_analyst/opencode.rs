use super::{
    AcpClient, SessionPurpose,
    acp::{PermissionPolicy, PromptCompletion},
    launch_command, process,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

mod command;
#[cfg(test)]
mod launch_tests;
mod lifecycle;
#[cfg(test)]
mod model_sync_tests;
mod session;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const START_ATTEMPTS: usize = 3;
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_MANAGED_VERSION: (u64, u64, u64) = (1, 18, 14);

pub(super) struct PreparedOpenCode {
    runtime: super::ActorRuntime,
    launch_prefix: Vec<String>,
    acp_arguments: Vec<String>,
    tui_arguments: Vec<String>,
    model: Option<String>,
    agent: Option<String>,
}

pub(super) struct LaunchedOpenCode {
    pub(super) protocol: AcpClient,
    pub(super) process: Arc<process::ChildOwner>,
    pub(super) session_id: String,
    pub(super) tui_command: Vec<String>,
    pub(super) environment: BTreeMap<String, String>,
    pub(super) resumed: bool,
}

pub(super) fn prepare(
    configured: &[String],
    environment: &BTreeMap<String, String>,
    runtime: super::ActorRuntime,
) -> io::Result<PreparedOpenCode> {
    let name = if runtime == super::ActorRuntime::Kilo {
        "kilo"
    } else {
        "opencode"
    };
    let default_command = [name.to_owned()];
    let configured = if configured.is_empty() {
        &default_command[..]
    } else {
        configured
    };
    let executable = launch_command::resolve_runtime_executable(&configured[0], environment)?;
    let launch_prefix = command::launch_prefix(&executable, name, environment)?;
    let parsed = command::parse_arguments(&configured[1..])?;
    Ok(PreparedOpenCode {
        runtime,
        launch_prefix,
        acp_arguments: parsed.acp_arguments,
        tui_arguments: parsed.tui_arguments,
        model: parsed.model,
        agent: parsed.agent,
    })
}

pub(super) async fn launch(
    prepared: PreparedOpenCode,
    cwd: &Path,
    base_environment: BTreeMap<String, String>,
    generation: &str,
    purpose: SessionPurpose,
    resume_session_id: Option<&str>,
    mcp_server: Value,
) -> io::Result<LaunchedOpenCode> {
    // Kilo shares this ACP/HTTP topology, not OpenCode's release numbering.
    if prepared.runtime == super::ActorRuntime::Opencode {
        require_supported_version(&prepared.launch_prefix[0], cwd, &base_environment).await?;
    }
    if purpose == SessionPurpose::VoiceAnalyst {
        command::write_voice_analyst_agent(
            cwd,
            super::launch::ANALYST_INSTRUCTIONS,
            prepared.runtime,
        )?;
    }
    let mut last_error = None;
    for _ in 0..START_ATTEMPTS {
        let port = lifecycle::reserve_loopback_port()?;
        let password = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let is_kilo = prepared.runtime == super::ActorRuntime::Kilo;
        let username = if is_kilo { "kilo" } else { "opencode" };
        let prefix = if is_kilo { "KILO" } else { "OPENCODE" };
        let endpoint = format!("http://127.0.0.1:{port}");
        let mut environment = base_environment.clone();
        environment.insert(format!("{prefix}_SERVER_USERNAME"), username.into());
        environment.insert(format!("{prefix}_SERVER_PASSWORD"), password.clone());
        if is_kilo {
            environment.insert("KILO_NO_DAEMON".into(), "1".into());
        }
        let mut acp_command = prepared.launch_prefix.clone();
        acp_command.push("acp".into());
        acp_command.extend(prepared.acp_arguments.clone());
        acp_command.extend([
            "--hostname".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--cwd".into(),
            cwd.to_string_lossy().into_owned(),
        ]);
        let (owner, stdin, stdout) = process::spawn_piped(
            &acp_command,
            cwd,
            &environment,
            if is_kilo { "kilo-acp" } else { "opencode-acp" },
        )?;
        let owner = Arc::new(owner);
        if let Err(error) = lifecycle::wait_for_authenticated_backend(
            &endpoint,
            username,
            &password,
            Arc::clone(&owner),
        )
        .await
        {
            last_error = Some(error.to_string());
            let _ = owner.stop();
            continue;
        }
        let protocol = AcpClient::new(
            stdin,
            stdout,
            generation.to_owned(),
            username,
            PermissionPolicy::AllowOnce,
            PromptCompletion::Response,
        )?;
        let initialized = protocol
            .request(
                "initialize",
                json!({
                    "protocolVersion":1,
                    "clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false}},
                    "clientInfo":{"name":"cccc","version":env!("CARGO_PKG_VERSION")}
                }),
                STARTUP_TIMEOUT,
            )
            .await;
        let initialized = match initialized {
            Ok(value)
                if value.get("protocolVersion") == Some(&json!(1))
                    && value.pointer("/agentCapabilities/loadSession")
                        == Some(&Value::Bool(true)) =>
            {
                value
            }
            Ok(_) => {
                protocol.close().await;
                let _ = owner.stop();
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "OpenCode/Kilo ACP does not advertise protocol v1 with loadSession support",
                ));
            }
            Err(error) => {
                protocol.close().await;
                let _ = owner.stop();
                return Err(error);
            }
        };
        drop(initialized);
        let desired_agent = if purpose == SessionPurpose::VoiceAnalyst {
            Some(command::VOICE_ANALYST_AGENT)
        } else {
            prepared.agent.as_deref()
        };
        let initialized_session = session::initialize(
            &protocol,
            cwd,
            resume_session_id,
            mcp_server,
            purpose == SessionPurpose::Actor,
            prepared.model.as_deref(),
            desired_agent,
        )
        .await;
        let (session_id, resumed) = match initialized_session {
            Ok(value) => value,
            Err(error) => {
                protocol.close().await;
                let _ = owner.stop();
                return Err(error);
            }
        };
        if let Err(error) =
            lifecycle::attach(&protocol, &endpoint, username, &password, &session_id).await
        {
            protocol.close().await;
            let _ = owner.stop();
            return Err(error);
        }
        let mut tui_command = prepared.launch_prefix.clone();
        tui_command.extend(["attach".into(), endpoint]);
        tui_command.extend(prepared.tui_arguments.clone());
        tui_command.extend([
            "--session".into(),
            session_id.clone(),
            "--dir".into(),
            cwd.to_string_lossy().into_owned(),
        ]);
        return Ok(LaunchedOpenCode {
            protocol,
            process: owner,
            session_id,
            tui_command,
            environment,
            resumed,
        });
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        format!(
            "OpenCode/Kilo authenticated backend could not start: {}",
            last_error.unwrap_or_else(|| "no loopback port was available".into())
        ),
    ))
}

async fn require_supported_version(
    executable: &str,
    cwd: &Path,
    environment: &BTreeMap<String, String>,
) -> io::Result<()> {
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--version")
        .current_dir(cwd)
        .envs(environment)
        .kill_on_drop(true);
    let output = tokio::time::timeout(VERSION_PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "OpenCode version probe timed out")
        })??;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "OpenCode version could not be verified for a managed ACP session",
        ));
    }
    let version = parse_version(&String::from_utf8_lossy(&output.stdout))
        .or_else(|| parse_version(&String::from_utf8_lossy(&output.stderr)))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "OpenCode returned an unrecognized version for a managed ACP session",
            )
        })?;
    if version < MIN_MANAGED_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "OpenCode {}.{}.{} cannot host a reliable managed ACP session; upgrade to 1.18.14 or newer",
                version.0, version.1, version.2
            ),
        ));
    }
    Ok(())
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word
            .trim_matches(|character: char| !character.is_ascii_digit() && character != '.')
            .split('.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_opencode_versions() {
        assert_eq!(parse_version("1.18.27"), Some((1, 18, 27)));
        assert_eq!(
            parse_version("opencode 1.18.14 (stable)"),
            Some(MIN_MANAGED_VERSION)
        );
        assert_eq!(parse_version("unknown"), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_versions_before_the_reliable_acp_completion_fence() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("tempdir");
        let executable = root.path().join("opencode");
        std::fs::write(&executable, "#!/bin/sh\necho 1.18.13\n").expect("old OpenCode");
        let mut permissions = executable.metadata().expect("metadata").permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions).expect("permissions");

        let error = require_supported_version(
            executable.to_str().expect("UTF-8 path"),
            root.path(),
            &BTreeMap::new(),
        )
        .await
        .expect_err("old OpenCode must fail closed");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("upgrade to 1.18.14 or newer"));

        std::fs::write(&executable, "#!/bin/sh\necho 1.18.14\n").expect("supported OpenCode");
        require_supported_version(
            executable.to_str().expect("UTF-8 path"),
            root.path(),
            &BTreeMap::new(),
        )
        .await
        .expect("minimum supported OpenCode");
    }
}
