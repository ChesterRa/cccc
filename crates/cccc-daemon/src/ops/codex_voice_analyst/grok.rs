use super::{
    AcpClient, SessionPurpose,
    acp::{PermissionPolicy, PromptCompletion},
    launch_command, process,
};
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const LEADER_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

mod command;
mod session;

pub(super) struct PreparedGrok {
    pub(super) leader_command: Vec<String>,
    pub(super) acp_command: Vec<String>,
    executable: String,
    tui_arguments: Vec<String>,
    rules: String,
    socket_path: PathBuf,
}

pub(super) struct LaunchedGrok {
    pub(super) protocol: AcpClient,
    pub(super) process: Arc<process::ChildOwner>,
    pub(super) auxiliary_processes: Vec<Arc<process::ChildOwner>>,
    pub(super) session_id: String,
    pub(super) tui_command: Vec<String>,
    pub(super) resumed: bool,
    pub(super) cleanup_paths: Vec<PathBuf>,
}

pub(super) fn prepare(
    home: &HomeLayout,
    configured: &[String],
    environment: &BTreeMap<String, String>,
    generation: &str,
) -> io::Result<PreparedGrok> {
    let default_command = ["grok".to_owned()];
    let configured = if configured.is_empty() {
        &default_command[..]
    } else {
        configured
    };
    let executable = launch_command::resolve_runtime_executable(&configured[0], environment)?;
    let filename = executable
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(filename.as_str(), "grok" | "grok.exe") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Grok managed sessions require the direct grok executable; wrappers and renamed binaries are not supported",
        ));
    }
    let parsed = command::parse_arguments(&configured[1..])?;
    let socket_dir = home.daemon_dir().join("grok-managed");
    std::fs::create_dir_all(&socket_dir)?;
    command::set_private_directory(&socket_dir)?;
    let socket_path = socket_dir.join(format!("{}.sock", &generation[..generation.len().min(20)]));
    if socket_path.to_string_lossy().len() > 96 && cfg!(unix) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "CCCC_HOME is too long for the Grok leader socket: {}",
                socket_path.display()
            ),
        ));
    }
    let executable = executable.to_string_lossy().into_owned();
    let socket = socket_path.to_string_lossy().into_owned();
    let mut leader_command = vec![executable.clone(), "agent".into()];
    leader_command.extend(parsed.agent_arguments.clone());
    leader_command.extend([
        "--always-approve".into(),
        "--leader-socket".into(),
        socket.clone(),
        "leader".into(),
        "--no-exit-on-disconnect".into(),
        "--relay-on-demand".into(),
        "--no-auto-update".into(),
    ]);
    let mut acp_command = vec![executable.clone(), "agent".into()];
    acp_command.extend(parsed.agent_arguments);
    acp_command.extend([
        "--always-approve".into(),
        "--leader".into(),
        "--leader-socket".into(),
        socket,
        "stdio".into(),
    ]);
    Ok(PreparedGrok {
        leader_command,
        acp_command,
        executable,
        tui_arguments: parsed.tui_arguments,
        rules: parsed.rules.join("\n\n"),
        socket_path,
    })
}

pub(super) async fn launch(
    prepared: PreparedGrok,
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    generation: &str,
    purpose: SessionPurpose,
    resume_session_id: Option<&str>,
    mcp_server: Value,
) -> io::Result<LaunchedGrok> {
    let leader = Arc::new(process::spawn_background(
        &prepared.leader_command,
        cwd,
        environment,
        "grok-leader",
    )?);
    let (protocol, acp_process) =
        match connect_acp(&prepared.acp_command, cwd, environment, generation, &leader).await {
            Ok(value) => value,
            Err(error) => {
                let _ = leader.stop();
                let _ = std::fs::remove_file(&prepared.socket_path);
                return Err(error);
            }
        };
    let result = session::initialize(
        &protocol,
        cwd,
        purpose,
        &prepared.rules,
        resume_session_id,
        mcp_server,
        purpose == SessionPurpose::Actor,
    )
    .await;
    let (session_id, resumed) = match result {
        Ok(value) => value,
        Err(error) => {
            protocol.close().await;
            let _ = acp_process.stop();
            let _ = leader.stop();
            let _ = std::fs::remove_file(&prepared.socket_path);
            return Err(error);
        }
    };
    let mut tui_command = vec![prepared.executable];
    tui_command.extend(prepared.tui_arguments);
    tui_command.extend([
        "--leader".into(),
        "--leader-socket".into(),
        prepared.socket_path.to_string_lossy().into_owned(),
        "--resume".into(),
        session_id.clone(),
        "--cwd".into(),
        cwd.to_string_lossy().into_owned(),
        "--always-approve".into(),
        "--no-alt-screen".into(),
    ]);
    Ok(LaunchedGrok {
        protocol,
        process: acp_process,
        auxiliary_processes: vec![leader],
        session_id,
        tui_command,
        resumed,
        cleanup_paths: vec![prepared.socket_path],
    })
}

async fn connect_acp(
    command: &[String],
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    generation: &str,
    leader: &Arc<process::ChildOwner>,
) -> io::Result<(AcpClient, Arc<process::ChildOwner>)> {
    let deadline = tokio::time::Instant::now() + LEADER_STARTUP_TIMEOUT;
    let mut last_error = None;
    while tokio::time::Instant::now() < deadline {
        if !leader.running() {
            return Err(io::Error::other("Grok leader exited during startup"));
        }
        let (owner, stdin, stdout) =
            match process::spawn_piped(command, cwd, environment, "grok-acp") {
                Ok(value) => value,
                Err(error) => {
                    last_error = Some(error.to_string());
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
        let owner = Arc::new(owner);
        let protocol = AcpClient::new(
            stdin,
            stdout,
            generation.to_owned(),
            "grok",
            PermissionPolicy::Reject,
            PromptCompletion::BoundedPostResponseDrain,
        )?;
        let initialized = protocol
            .request(
                "initialize",
                json!({
                    "protocolVersion":1,
                    "clientCapabilities":{
                        "fs":{"readTextFile":false,"writeTextFile":false}
                    },
                    "clientInfo":{"name":"cccc","version":env!("CARGO_PKG_VERSION")}
                }),
                Duration::from_secs(3),
            )
            .await;
        match initialized {
            Ok(result)
                if result.get("protocolVersion") == Some(&json!(1))
                    && result.pointer("/agentCapabilities/loadSession")
                        == Some(&Value::Bool(true)) =>
            {
                return Ok((protocol, owner));
            }
            Ok(_) => {
                last_error =
                    Some("Grok ACP does not advertise protocol v1 with loadSession support".into());
            }
            Err(error) => last_error = Some(error.to_string()),
        }
        protocol.close().await;
        let _ = owner.stop();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "Grok leader did not become ready for ACP: {}",
            last_error.unwrap_or_else(|| "startup timeout".into())
        ),
    ))
}
