mod command;
mod manager;
mod output;
mod session;

pub use command::{default_command, detect_runtimes};
pub use manager::{
    bracketed_paste_enabled, clear, history, history_since, reap, resize, start, status, stop,
    stop_all, write,
};
pub use session::{LaunchSpec, SessionStatus};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime session already exists: {0}/{1}")]
    AlreadyRunning(String, String),
    #[error("runtime session not found: {0}/{1}")]
    NotFound(String, String),
    #[error("runtime command is empty")]
    EmptyCommand,
    #[error("runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime state lock is poisoned")]
    Poisoned,
}
