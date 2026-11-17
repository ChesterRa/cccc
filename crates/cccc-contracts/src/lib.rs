#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
pub mod actor;
pub mod backlog;
pub mod chet;
pub mod codex_voice;
pub mod connect;
pub mod connect_groups;
pub mod connect_message;
pub mod deepseek;
pub mod direct;
pub mod event;
pub mod ipc;
pub mod local_session;
pub mod message;
pub mod message_args;
pub mod message_pins;
pub mod reply_question;
pub mod voice_notifications;
pub mod voice_secretary;

pub use actor::{
    Actor, ActorRole, ActorRuntime, ActorSubmit, GroupState, RunnerKind, RuntimeMode,
    RuntimeStateSource,
};
pub use codex_voice::{AgentRuntimeSettings, CodexVoiceAnalystSettings, CodexVoiceSettings};
pub use deepseek::{
    DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION, DEEPSEEK_ACP_PACKAGE,
    DEEPSEEK_ACP_SDK_VERSION, DEEPSEEK_ACP_VERSION, DEEPSEEK_DEFAULT_MODEL,
    DEEPSEEK_LLM_ADAPTER_PACKAGE, DEEPSEEK_LLM_ADAPTER_VERSION, DEEPSEEK_MAX_OUTPUT_TOKENS,
    DEEPSEEK_MCP_CLIENT_PACKAGE, DEEPSEEK_MCP_CLIENT_VERSION, DEEPSEEK_MODEL_ENV,
    DEEPSEEK_NODE_RANGE, DEEPSEEK_NPM_BEFORE, DEEPSEEK_PROTOCOL_VERSION, DEEPSEEK_RELEASE_VERSION,
    DEEPSEEK_TURN_TIMEOUT_SECONDS,
};
pub use event::Event;
pub use ipc::{DaemonAddress, DaemonError, DaemonRequest, DaemonResponse, Transport};
pub use message::{Attachment, ChatMessageData, ChatStreamData, MessageMode, Reference};

pub const RUST_DAEMON_COMPATIBILITY: &str = "cccc-rust-daemon-v2";

/// The daemon behavior used by CCCC MCP tools. Breaking an existing tool's
/// operation, fields or semantics requires a new contract; additions do not.
/// This is separate from the Rust daemon's implementation-generation marker.
pub const MCP_DAEMON_CONTRACT: u32 = 1;

/// Tool contracts this daemon actually implements. This is the complete
/// accepted set, distinct from the contract emitted by a new MCP process.
/// Removing one prevents preserving clients through an update from a daemon
/// that still accepts it; adding one requires implementing its semantics.
pub const MCP_DAEMON_ACCEPTED_CONTRACTS: &[u32] = &[1];

pub fn utc_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}
