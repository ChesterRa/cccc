//! Process-local lifecycle facts shared with the terminal interface.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DaemonState {
    StartedHere { pid: u32 },
    External { pid: Option<u32> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WebState {
    Starting,
    Listening { url: String },
    Restarting,
    Stopped,
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StopResult {
    Stopped,
    KeptExternal { pid: Option<u32> },
    Failed { message: String },
}

/// What the daemon's exit receipt confirmed about its agents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentsResult {
    /// The daemon is not this launch's, so neither are its agents.
    NotOwned,
    /// Counts from the receipt; `uncertain` agents may still be running. `review_owners`
    /// are the session owners kept for review because they could not confirm one.
    Confirmed {
        stopped: usize,
        kept: usize,
        uncertain: usize,
        review_owners: Vec<String>,
    },
    /// No receipt for this exit: nothing about the agents is confirmed.
    Unconfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShutdownReport {
    pub web: StopResult,
    pub daemon: StopResult,
    pub agents: AgentsResult,
    pub log_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HostPhase {
    Running,
    Stopping,
    /// An attended update is replacing this process with `version`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Updating {
        version: String,
    },
    Stopped {
        report: ShutdownReport,
    },
    /// This process is about to be replaced; `message` says by what.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Replaced {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HostState {
    pub daemon: DaemonState,
    pub web: WebState,
    pub phase: HostPhase,
    /// How the attended update that started this process ended, once known.
    pub update: Option<UpdateResult>,
}

/// One attended update's result, as this process can tell it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateResult {
    /// The update this is the result of, so a repeated failure is still new.
    pub transaction: String,
    pub failed: bool,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostIntent {
    Quit,
    Interrupt,
}
