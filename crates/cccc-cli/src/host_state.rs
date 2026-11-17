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
    Stopped { report: ShutdownReport },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HostState {
    pub daemon: DaemonState,
    pub web: WebState,
    pub phase: HostPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostIntent {
    Quit,
    Interrupt,
}
