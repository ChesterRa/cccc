use cccc_core::HomeLayout;
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::broadcast;

mod launch;
mod process;
mod protocol;
#[cfg(test)]
mod tests;
mod turns;

use process::ChildOwner;
use protocol::ProtocolClient;

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    pub group_id: String,
    pub root: PathBuf,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub resume_thread_id: Option<String>,
    pub codex_executable: Option<PathBuf>,
}

impl LaunchConfig {
    pub fn new(group_id: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            group_id: group_id.into(),
            root: root.into(),
            model: None,
            profile: None,
            resume_thread_id: None,
            codex_executable: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeBinding {
    pub group_id: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AnalystEvent {
    pub generation: String,
    pub message: Value,
    pub requested_delegation_id: Option<String>,
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
    binding: ScopeBinding,
    generation: String,
    endpoint: String,
    thread_id: String,
    codex_executable: PathBuf,
    protocol: ProtocolClient,
    process: Option<Arc<ChildOwner>>,
    thread_materialized: AtomicBool,
    delegations: tokio::sync::Mutex<HashMap<String, DelegationState>>,
}

struct ConnectConfig {
    binding: ScopeBinding,
    generation: String,
    endpoint: String,
    codex_executable: PathBuf,
    model: Option<String>,
    resume_thread_id: Option<String>,
    process: Option<Arc<ChildOwner>>,
    delegations: HashMap<String, DelegationState>,
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
