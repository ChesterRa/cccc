mod cancellation;
mod command;
mod history_access;
mod manager;
mod output;
mod output_reader;
mod registry;
mod session;
mod session_history;
mod transcript_archive;
mod transcript_files;
mod transcript_reader;

pub use command::{default_command, detect_runtimes};
pub use history_access::{
    active_history_replay, active_history_since, bracketed_paste_enabled, clear, history,
    history_since, retained_history, retained_history_tail,
};
pub use manager::{
    reap, resize, start, start_with_history, status, stop, stop_all, stop_if_started_at, submit,
    submit_interruptible, submit_sequence_interruptible, write,
};
pub use output::HistoryPage;
pub use session::{LaunchSpec, SessionStatus};
pub use transcript_archive::HistoryConfig;
pub use transcript_reader::{read_latest_page, read_latest_since};

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
