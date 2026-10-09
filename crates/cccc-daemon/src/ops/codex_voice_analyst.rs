use cccc_contracts::{ActorRuntime, RuntimeMode};
use cccc_core::HomeLayout;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

mod acp;
mod antigravity;
mod claude;
mod control;
mod grok;
mod launch;
pub(crate) use launch::ANALYST_INSTRUCTIONS;
pub(crate) use launch_secretary::{
    INSTRUCTIONS as SECRETARY_INSTRUCTIONS, cleanup_secretary_runtime, secretary_cleanup_pending,
};
mod launch_antigravity;
mod native_acp;
pub(crate) use native_acp::{name as native_acp_name, valid_id as valid_native_acp_id};
mod launch_claude;
mod launch_codex;
mod launch_command;
mod launch_grok;
mod launch_native_acp;
mod launch_opencode;
mod launch_secretary;
pub(crate) use launch_antigravity::login_antigravity;
pub(crate) mod lifecycle_timing;
mod native_input;
mod opencode;
mod process;
mod protocol;
#[cfg(test)]
mod tests;
mod turns;

pub(crate) fn managed_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("cccc-managed-agent")
            .enable_all()
            .build()
            .expect("build shared managed Agent runtime")
    })
}

/// The workspace Claude Code refused to launch in because its trust prompt was never accepted.
pub(crate) fn untrusted_claude_workspace(error: &io::Error) -> Option<&std::path::Path> {
    claude::untrusted_workspace(error)
}

#[cfg(test)]
pub(crate) fn claude_workspace_refusal(
    detail: &str,
    workspace: &std::path::Path,
) -> Option<io::Error> {
    claude::workspace_refusal(detail, workspace)
}

pub(crate) fn remove_claude_actor_settings(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> io::Result<()> {
    claude::remove_actor_settings(home, group_id, actor_id)
}

use acp::AcpClient;
use claude::ClaudeClient;
use process::ChildOwner;
use protocol::ProtocolClient;

pub(crate) const MANAGED_AGENT_DISCONNECTED_METHOD: &str = "cccc/managedAgent/disconnected";
pub(crate) const MANAGED_AGENT_DELEGATION_ATTACHED_METHOD: &str =
    "cccc/managedAgent/delegationAttached";
const CODEX_TURN_CORRELATION_KEY: &str = "cccc_turn_correlation_id";

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub workdir: PathBuf,
    pub runtime: ActorRuntime,
    pub runtime_mode: RuntimeMode,
    pub command: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub resume_thread_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActorLaunchConfig {
    pub(crate) workdir: PathBuf,
    pub(crate) group_id: String,
    pub(crate) actor_id: String,
    pub(crate) runtime: ActorRuntime,
    pub(crate) runtime_mode: RuntimeMode,
    pub(crate) command: Vec<String>,
    pub(crate) environment: BTreeMap<String, String>,
}

impl LaunchConfig {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            runtime: ActorRuntime::Codex,
            runtime_mode: RuntimeMode::Default,
            command: Vec::new(),
            environment: BTreeMap::new(),
            resume_thread_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceBinding {
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AnalystEvent {
    pub generation: String,
    pub message: Value,
    pub requested_delegation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPurpose {
    VoiceAnalyst,
    Actor,
    VoiceSecretary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnReceipt {
    pub delegation_id: String,
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug)]
enum DelegationState {
    Started(TurnReceipt),
    Unresolved(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElicitationAction {
    Accept,
    Decline,
}

impl ElicitationAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Decline => "decline",
        }
    }
}

pub(crate) struct AnalystSession {
    #[cfg(test)]
    binding: WorkspaceBinding,
    generation: String,
    runtime: ActorRuntime,
    endpoint: String,
    thread_id: String,
    remote_tui_prefix: Vec<String>,
    environment: BTreeMap<String, String>,
    protocol: ManagedProtocol,
    process: Option<Arc<ChildOwner>>,
    auxiliary_processes: Vec<Arc<ChildOwner>>,
    native_tui_command: Option<Vec<String>>,
    cleanup_paths: Vec<PathBuf>,
    thread_resumed: bool,
    delegations: tokio::sync::Mutex<HashMap<String, DelegationState>>,
}

/// Failed startup still owns its process until cleanup is confirmed. Keeping
/// this error alive lets the caller retain capacity and retry cleanup at stop.
#[derive(Clone)]
pub(crate) struct PendingManagedStartup(Arc<ChildOwner>);

impl PendingManagedStartup {
    pub(crate) fn from_error(error: &io::Error) -> Option<Self> {
        error
            .get_ref()?
            .downcast_ref::<StartupCleanupFailure>()
            .map(|failure| failure.owner.clone())
    }
    pub(crate) fn stop(&self) -> io::Result<()> {
        self.0.stop()
    }
}

struct StartupCleanupFailure {
    owner: PendingManagedStartup,
    startup: io::Error,
    cleanup: io::Error,
}
impl std::fmt::Debug for StartupCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::fmt::Display for StartupCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}; provider cleanup is unresolved: {}",
            self.startup, self.cleanup
        )
    }
}
impl std::error::Error for StartupCleanupFailure {}

enum ManagedProtocol {
    Codex(ProtocolClient),
    Acp(AcpClient),
    Claude(ClaudeClient),
}

/// Select the task authority surface without putting grants in MCP descriptors.
fn task_tool_profile(purpose: SessionPurpose, ordinary: Option<&str>) -> Option<&str> {
    if purpose == SessionPurpose::VoiceSecretary {
        Some("secretary-task")
    } else {
        ordinary
    }
}

fn acp_mcp_server(
    home: &HomeLayout,
    executable: &std::path::Path,
    group_id: &str,
    actor_id: &str,
    tool_profile: Option<&str>,
) -> Value {
    let mut environment = vec![
        serde_json::json!({"name":"CCCC_HOME","value":home.root().to_string_lossy()}),
        serde_json::json!({"name":"CCCC_GROUP_ID","value":group_id}),
        serde_json::json!({"name":"CCCC_ACTOR_ID","value":actor_id}),
    ];
    if let Some(tool_profile) = tool_profile {
        environment.push(serde_json::json!({"name":"CCCC_MCP_TOOL_PROFILE","value":tool_profile}));
    }
    serde_json::json!({
        "name":"cccc",
        "command":executable.to_string_lossy(),
        "args":["mcp"],
        "env":environment,
    })
}

fn add_voice_mcp_origin(
    server: &mut Value,
    environment: &BTreeMap<String, String>,
    purpose: SessionPurpose,
) {
    if purpose == SessionPurpose::VoiceSecretary
        && let Some(path) = environment.get("CCCC_SECRETARY_TASK_TOKEN_FILE")
        && let Some(env) = server["env"].as_array_mut()
    {
        env.push(serde_json::json!({"name":"CCCC_SECRETARY_TASK_TOKEN_FILE","value":path}));
    }
    if purpose == SessionPurpose::VoiceAnalyst
        && let Some(origin) = environment.get(cccc_core::voice_notifications::ORIGIN_ENV)
        && let Some(env) = server["env"].as_array_mut()
    {
        env.push(
            serde_json::json!({"name":cccc_core::voice_notifications::ORIGIN_ENV,"value":origin}),
        );
    }
}

impl ManagedProtocol {
    fn subscribe(&self) -> broadcast::Receiver<AnalystEvent> {
        match self {
            Self::Codex(protocol) => protocol.subscribe(),
            Self::Acp(protocol) => protocol.subscribe(),
            Self::Claude(protocol) => protocol.subscribe(),
        }
    }

    async fn respond(&self, id: Value, result: Value) -> io::Result<()> {
        match self {
            Self::Codex(protocol) => protocol.respond(id, result).await,
            Self::Acp(protocol) => protocol.respond(id, result).await,
            Self::Claude(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Claude Agent View does not expose generic JSON-RPC responses",
            )),
        }
    }

    async fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        match self {
            Self::Codex(protocol) => protocol.respond_error(id, error).await,
            Self::Acp(protocol) => protocol.respond_error(id, error).await,
            Self::Claude(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Claude Agent View does not expose generic JSON-RPC responses",
            )),
        }
    }

    async fn close(&self) -> io::Result<()> {
        match self {
            Self::Codex(protocol) => {
                protocol.close().await;
                Ok(())
            }
            Self::Acp(protocol) => {
                protocol.close().await;
                Ok(())
            }
            Self::Claude(protocol) => protocol.close().await,
        }
    }

    /// Best-effort stop request for sessions that are not owned process trees.
    /// Codex and ACP providers are child processes and die with this process.
    async fn kill_request(&self) -> io::Result<()> {
        match self {
            Self::Codex(_) | Self::Acp(_) => Ok(()),
            Self::Claude(protocol) => protocol.kill_request().await,
        }
    }

    async fn register_native_input(&self, delegation_id: &str, text: &str) -> io::Result<()> {
        match self {
            // Codex uses exact turn/steer whenever an active turn id exists. The only native-input
            // fallback is the short thread-start admission window, whose next turn is correlated
            // by the lifecycle owner.
            Self::Codex(_) => Ok(()),
            Self::Acp(protocol) => protocol.register_native_input(delegation_id, text).await,
            Self::Claude(protocol) => protocol.register_native_input(delegation_id, text).await,
        }
    }

    async fn forget_native_input(&self, delegation_id: &str) -> io::Result<()> {
        match self {
            Self::Codex(_) => Ok(()),
            Self::Acp(protocol) => protocol.forget_native_input(delegation_id).await,
            Self::Claude(protocol) => protocol.forget_native_input(delegation_id).await,
        }
    }

    fn running(&self) -> bool {
        match self {
            Self::Claude(protocol) => protocol.running(),
            Self::Codex(_) | Self::Acp(_) => true,
        }
    }

    #[cfg(test)]
    fn publish_for_test(&self, event: AnalystEvent) {
        match self {
            Self::Codex(protocol) => {
                let _ = protocol.events.send(event);
            }
            Self::Acp(protocol) => {
                let _ = protocol.events.send(event);
            }
            Self::Claude(protocol) => {
                let _ = protocol.events.send(event);
            }
        }
    }
}

struct ConnectConfig {
    binding: WorkspaceBinding,
    generation: String,
    endpoint: String,
    remote_tui_prefix: Vec<String>,
    environment: BTreeMap<String, String>,
    resume_thread_id: Option<String>,
    process: Option<Arc<ChildOwner>>,
    delegations: HashMap<String, DelegationState>,
    purpose: SessionPurpose,
}

fn required_value<'a>(value: &'a str, name: &str) -> io::Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} is required"),
        ))
    } else {
        Ok(value)
    }
}
