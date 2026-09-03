use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::broadcast;

mod acp;
mod control;
mod grok;
mod launch;
mod launch_codex;
mod launch_command;
mod launch_grok;
mod launch_opencode;
mod opencode;
mod process;
mod protocol;
#[cfg(test)]
mod tests;
mod turns;

use acp::AcpClient;
use process::ChildOwner;
use protocol::ProtocolClient;

pub(crate) const MANAGED_AGENT_DISCONNECTED_METHOD: &str = "cccc/managedAgent/disconnected";
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
    pub(crate) runner: cccc_contracts::RunnerKind,
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
    thread_materialized: AtomicBool,
    thread_resumed: bool,
    delegations: tokio::sync::Mutex<HashMap<String, DelegationState>>,
}

enum ManagedProtocol {
    Codex(ProtocolClient),
    Acp(AcpClient),
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
        }
    }

    async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: std::time::Duration,
    ) -> io::Result<Value> {
        match self {
            Self::Codex(protocol) => protocol.request(method, params, timeout).await,
            Self::Acp(protocol) => protocol.request(method, params, timeout).await,
        }
    }

    async fn respond(&self, id: Value, result: Value) -> io::Result<()> {
        match self {
            Self::Codex(protocol) => protocol.respond(id, result).await,
            Self::Acp(protocol) => protocol.respond(id, result).await,
        }
    }

    async fn respond_error(&self, id: Value, error: Value) -> io::Result<()> {
        match self {
            Self::Codex(protocol) => protocol.respond_error(id, error).await,
            Self::Acp(protocol) => protocol.respond_error(id, error).await,
        }
    }

    async fn close(&self) {
        match self {
            Self::Codex(protocol) => protocol.close().await,
            Self::Acp(protocol) => protocol.close().await,
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
