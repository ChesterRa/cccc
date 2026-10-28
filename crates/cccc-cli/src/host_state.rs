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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShutdownReport {
    pub web: StopResult,
    pub daemon: StopResult,
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
