use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

mod acp;
mod claude;
mod control;
mod grok;
mod launch;
mod launch_claude;
mod launch_codex;
mod launch_command;
mod launch_grok;
mod launch_opencode;
pub(crate) mod lifecycle_timing;
mod native_input;
mod opencode;
mod process;
mod protocol;
#[cfg(test)]
mod tests;
mod turns;

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
    pub(crate) command: Vec<String>,
    pub(crate) environment: BTreeMap<String, String>,
}

impl LaunchConfig {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            runtime: ActorRuntime::Codex,
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

enum ManagedProtocol {
    Codex(ProtocolClient),
    Acp(AcpClient),
    Claude(ClaudeClient),
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
