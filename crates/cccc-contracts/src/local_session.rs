//! User-owned native sessions. A receiver connection can outlive a conversation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LocalSessionRuntime {
    Codex,
    Claude,
    Pi,
    Opencode,
}

impl LocalSessionRuntime {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Pi => "pi",
            Self::Opencode => "opencode",
        }
    }

    pub fn source_id(self, native_session_id: &str) -> String {
        format!("{}:{native_session_id}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSessionSource {
    pub source_id: String,
    pub runtime: LocalSessionRuntime,
    pub native_session_id: String,
    pub cwd: String,
    pub title: String,
    /// Optional native creation origin (cli/vscode); display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Current receive capability (native queue or Monitor lease), not proof
    /// that a terminal window is open or that Group access is enabled.
    pub connected: bool,
}

/// Trusted MCP request snapshot, checked again under the daemon operation permit.
/// Routing identifiers do not themselves grant authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSessionSourceBinding {
    pub source_id: String,
    pub session_revision: String,
}

/// Group authority additionally captures the receiving membership generation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSessionBinding {
    pub group_id: String,
    pub actor_id: String,
    pub actor_generation: String,
    pub source_id: String,
    pub session_revision: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalSessionRecoveryState {
    Connected,
    Disconnected,
    HostExited,
    SessionEnded,
    IntegrationDisabled,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalSessionRefreshStatus {
    Checked,
    Unavailable,
    NotChecked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LocalSessionShell {
    Posix,
    PowerShell,
}

/// Private administrator projection; never part of ordinary Actor listings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSessionRecovery {
    pub state: LocalSessionRecoveryState,
    pub cwd: Option<String>,
    pub shell: LocalSessionShell,
    /// Manually run with the indicated shell on the original CCCC host; never
    /// executed by CCCC. Includes only whitelisted paths and a known native ID.
    pub command: String,
}
