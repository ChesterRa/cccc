use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::broadcast;

mod launch;
mod launch_command;
mod process;
mod protocol;
#[cfg(test)]
mod tests;
mod turns;

use process::ChildOwner;
use protocol::ProtocolClient;

pub(crate) const CODEX_APP_DISCONNECTED_METHOD: &str = "cccc/codexApp/disconnected";
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
    endpoint: String,
    thread_id: String,
    remote_tui_prefix: Vec<String>,
    environment: BTreeMap<String, String>,
    protocol: ProtocolClient,
    process: Option<Arc<ChildOwner>>,
    thread_materialized: AtomicBool,
    thread_resumed: bool,
    delegations: tokio::sync::Mutex<HashMap<String, DelegationState>>,
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
