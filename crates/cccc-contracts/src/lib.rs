pub mod actor;
pub mod event;
pub mod ipc;
pub mod message;

pub use actor::{
    Actor, ActorRole, ActorRuntime, ActorSubmit, GroupState, RunnerKind, RuntimeStateSource,
};
pub use event::Event;
pub use ipc::{DaemonAddress, DaemonError, DaemonRequest, DaemonResponse, Transport};

pub const RUST_DAEMON_COMPATIBILITY: &str = "cccc-rust-daemon-v2";
pub use message::{Attachment, ChatMessageData, ChatStreamData, Reference};

pub fn utc_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}
