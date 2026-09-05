mod cancellation;
mod command;
pub mod deepseek_acp;
pub mod deepseek_supervisor;
mod executable;
mod history_access;
mod manager;
#[cfg(all(test, windows))]
mod manager_windows_tests;
mod output;
mod output_reader;
mod process_tree;
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
mod terminal_query_responder;
mod terminal_response_writer;
mod terminal_sequence_tracker;
mod terminal_snapshot;
#[cfg(test)]
mod test_support;
mod transcript_archive;
mod transcript_files;
mod transcript_reader;

pub use command::{
    DEEPSEEK_ACP_APP_PACKAGE, DEEPSEEK_ACP_APP_VERSION, DEEPSEEK_ACP_PACKAGE,
    DEEPSEEK_ACP_SDK_VERSION, DEEPSEEK_ACP_VERSION, DEEPSEEK_LLM_ADAPTER_PACKAGE,
    DEEPSEEK_LLM_ADAPTER_VERSION, DEEPSEEK_MAX_OUTPUT_TOKENS, DEEPSEEK_MCP_CLIENT_PACKAGE,
    DEEPSEEK_MCP_CLIENT_VERSION, DEEPSEEK_NODE_RANGE, DEEPSEEK_NPM_BEFORE,
    DEEPSEEK_RELEASE_VERSION, DEEPSEEK_TURN_TIMEOUT_SECONDS, canonical_deepseek_runtime_manifest,
    deepseek_bootstrap_preflight, deepseek_external_preflight, deepseek_home,
    deepseek_lockfile_is_pinned, deepseek_preflight, default_command, detect_runtimes,
    is_canonical_deepseek_config, is_canonical_deepseek_profile_manifest,
    is_canonical_deepseek_runtime_manifest,
};
pub use executable::{prepare_pty_command, resolve_command_executable, resolve_executable_in_path};
pub use history_access::{
    active_history_replay, active_history_since, bracketed_paste_enabled, clear, history,
    history_since, retained_history, retained_history_tail,
};
pub use manager::{
    reap, resize, start, start_with_history, status, stop, stop_all, stop_if_started_at, submit,
    submit_interruptible, submit_sequence_interruptible, wait_for_input_ready, write,
};
pub use output::HistoryPage;
pub use process_tree::{OwnedProcessTree, force_terminate_owned};
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
