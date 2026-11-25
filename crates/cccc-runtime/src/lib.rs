#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
mod cancellation;
mod command;
mod command_output;
mod command_session;
mod diagnostic;
pub use diagnostic::{DiagnosticLog, diagnostic, log_diagnostics_to};
#[cfg(target_os = "macos")]
pub mod darwin_process;
pub mod deepseek_acp;
pub mod deepseek_supervisor;
mod executable;
mod history_access;
mod manager;
mod native_codex;
pub mod process_children;
pub mod process_recovery;
pub use native_codex::{
    NativeCodexSessionInfo, NativeCodexSteerOutcome, native_codex_can_receive,
    native_codex_receive_state, native_codex_session_info, native_codex_try_steer,
};
#[cfg(all(test, windows))]
mod manager_windows_tests;
mod output;
mod output_reader;
mod path_form;
mod process_tree;
mod pty_input;
#[cfg(target_os = "linux")]
mod pty_io;
mod registry;
pub mod running_image;
mod session;
mod session_history;
#[cfg(test)]
mod session_history_tests;
mod startup_input;
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
    DEEPSEEK_ACP_SDK_VERSION, DEEPSEEK_MAX_OUTPUT_TOKENS, DEEPSEEK_NODE_RANGE, DEEPSEEK_NPM_BEFORE,
    DEEPSEEK_PACKAGE, DEEPSEEK_RELEASE_VERSION, DEEPSEEK_TURN_TIMEOUT_SECONDS,
    canonical_deepseek_runtime_manifest, deepseek_acp_profile, deepseek_executable,
    deepseek_external_preflight, deepseek_home, deepseek_installation_ready,
    deepseek_managed_command, deepseek_packages_to_reinstall, default_command, detect_runtimes,
    unicode_environment,
};
pub use command_output::{
    CapturedOutput, capture_command, capture_command_blocking, capture_command_with_start_evidence,
};
pub use command_session::CommandSession;
pub use executable::{prepare_pty_command, resolve_command_executable, resolve_executable_in_path};
pub use history_access::{
    active_history_replay, active_history_since, bracketed_paste_enabled, clear, history,
    history_since, last_output, retained_history, retained_history_tail,
};
pub use manager::{
    PtyHandoff, PtyHandoffReason, reap, release_startup_input, reset_exited_startup_input, resize,
    start, start_actor_with_history, start_with_history, start_with_history_isolated_environment,
    status, stop, stop_all, stop_if_started_at, submit, submit_interruptible,
    submit_sequence_for_new_kilo_viewer, submit_sequence_for_session_interruptible,
    submit_sequence_for_session_with_evidence_interruptible, submit_sequence_interruptible,
    wait_for_input_ready, wait_for_new_kilo_composer, write, write_to_session_within,
};
pub use output::HistoryPage;
pub use path_form::compatible_path;
#[doc(hidden)]
#[cfg(all(target_os = "macos", debug_assertions))]
pub use process_tree::run_process_watchdog_test_child;
pub use process_tree::{
    CommandError, OwnedProcessTree, ProcessStartUnconfirmed, background_command,
    background_tokio_command, force_terminate_owned, owned_process_recovery_pending,
    run_process_watchdog_from_args,
};

#[cfg(test)]
crate::process_watchdog_test_entry!();
#[cfg(unix)]
pub use process_tree::{
    DetachedWork, protect_detached_work, recover_inactive_owned_process_groups,
};
#[cfg(unix)]
pub use process_tree::{prepare_web_process_guard, protect_owned_process_groups};
pub use session::{ActorLaunch, LaunchSpec, SessionStatus};
pub use session_history::OutputObservation;
pub use startup_input::{
    required as startup_input_required, resolve_antigravity_home, resolve_antigravity_home_from,
};
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
    #[error(
        "cmd.exe cannot run in {0}: it cannot use a UNC path, or a path that needs the \\\\?\\ prefix, as its current directory, and would start in the Windows directory or not at all. Use a workspace on a local disk with an ordinary path, or a program that does not start through cmd.exe"
    )]
    CmdInPrefixedDirectory(String),
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
