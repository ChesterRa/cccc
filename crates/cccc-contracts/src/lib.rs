pub mod actor;
pub mod deepseek;
pub mod event;
pub mod ipc;
pub mod message;

pub use actor::{
    Actor, ActorRole, ActorRuntime, ActorSubmit, GroupState, RunnerKind, RuntimeStateSource,
};
pub use deepseek::{
    DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION, DEEPSEEK_ACP_PACKAGE,
    DEEPSEEK_ACP_SDK_VERSION, DEEPSEEK_ACP_VERSION, DEEPSEEK_DSH_PACKAGE, DEEPSEEK_DSH_VERSION,
    DEEPSEEK_LLM_ADAPTER_PACKAGE, DEEPSEEK_LLM_ADAPTER_VERSION, DEEPSEEK_MCP_CLIENT_PACKAGE,
    DEEPSEEK_MCP_CLIENT_VERSION, DEEPSEEK_NODE_RANGE, DEEPSEEK_PROTOCOL_VERSION,
};
pub use event::Event;
pub use ipc::{DaemonAddress, DaemonError, DaemonRequest, DaemonResponse, Transport};

pub const RUST_DAEMON_COMPATIBILITY: &str = "cccc-rust-daemon-v2";
pub use message::{Attachment, ChatMessageData, ChatStreamData, Reference};

pub fn utc_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}
