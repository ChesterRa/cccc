mod cancellation;
mod command;
mod history_access;
mod manager;
mod output;
mod output_reader;
mod registry;
mod session;
mod session_history;
#[cfg(test)]
mod session_history_tests;
mod terminal_attach;
mod terminal_attachment_registry;
mod terminal_initial_output;
mod terminal_manager;
#[cfg(all(test, unix))]
mod terminal_manager_tests;
mod terminal_modes;
mod terminal_sequence_tracker;
mod terminal_snapshot;
#[cfg(test)]
mod test_support;
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
pub use terminal_attach::{TerminalAttachMode, TerminalAttachment, TerminalInput, TerminalOutput};
pub use terminal_initial_output::{TerminalInitialOutput, TerminalInitialOutputKind};
pub use terminal_manager::{
    attach, attach_with_size, attach_with_snapshot, attach_with_snapshot_and_size,
    attachment_writable, resize_from_attachment,
};
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
    #[error("runtime session is not running: {0}/{1}")]
    NotRunning(String, String),
    #[error(
        "terminal output cursor {requested} expired; retained output starts at {retained_start}"
    )]
    OutputLagged { requested: u64, retained_start: u64 },
    #[error("runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime state lock is poisoned")]
    Poisoned,
}
