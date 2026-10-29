//! The work behind `/setup`: checking the four native apps, then connecting
//! the ones the user picked, one at a time. It runs as a Tokio task fed by
//! the interface thread, and stops when the host stops it.

use crate::commands::sessions::setup::{self, SetupInspection};
use cccc_contracts::local_session::LocalSessionRuntime;
use cccc_core::HomeLayout;
use std::path::PathBuf;
use tokio::sync::{mpsc, watch};

/// The apps, in the order the flow lists and installs them.
pub(super) const RUNTIMES: [LocalSessionRuntime; 4] = [
    LocalSessionRuntime::Codex,
    LocalSessionRuntime::Claude,
    LocalSessionRuntime::Pi,
    LocalSessionRuntime::Opencode,
];

/// Whether an app can be picked: the confirmation must name the directory its
/// installer will write, so an app whose directory is unknown cannot be.
pub(super) fn pickable(check: &SetupInspection) -> bool {
    check.executable.is_some() && check.provider_dir.is_some()
}

/// Each `/setup` visit has its own generation, so progress left over from
/// an earlier visit is never read as this one's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SetupCommand {
    Check {
        generation: u64,
    },
    Install {
        generation: u64,
        runtimes: Vec<LocalSessionRuntime>,
    },
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum InstallState {
    Waiting,
    Installing,
    /// The app's configuration was written where the installer reports.
    /// Whether open sessions load it is up to the app, so this is not a
    /// claim that anything is connected.
    Written {
        provider_dir: PathBuf,
        warnings: Vec<String>,
    },
    Failed(String),
    /// Stopped while the app's installer ran, so part of it may be written.
    Interrupted,
    /// Never started because the flow was cancelled first.
    Skipped,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SetupProgress {
    pub(super) generation: u64,
    pub(super) checking: bool,
    /// One entry per app, in [`RUNTIMES`] order, once checked.
    pub(super) checks: Vec<SetupInspection>,
    pub(super) installs: Vec<(LocalSessionRuntime, InstallState)>,
}

impl SetupProgress {
    pub(super) fn installing(&self) -> bool {
        self.installs
            .iter()
            .any(|(_, state)| matches!(state, InstallState::Waiting | InstallState::Installing))
    }
}

/// Serve the interface's setup commands until `stop` turns true or the
/// interface goes away. A stop that arrives during an install interrupts it
/// and records that before returning.
pub(super) async fn run(
    home: HomeLayout,
    mut commands: mpsc::UnboundedReceiver<SetupCommand>,
    progress: watch::Sender<SetupProgress>,
    mut stop: watch::Receiver<bool>,
) {
    let mut next = None;
    loop {
        let command = match next.take() {
            Some(command) => command,
            None => tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => command,
                    None => return,
                },
                _ = stop.wait_for(|stop| *stop) => return,
            },
        };
        match command {
            SetupCommand::Check { generation } => {
                progress.send_replace(SetupProgress {
                    generation,
                    checking: true,
                    ..SetupProgress::default()
                });
                // Leaving the flow, or opening it again, ends a slow check
                // rather than waiting it out; dropping it ends its commands.
                tokio::select! {
                    checks = check(&home) => progress.send_modify(|progress| {
                        progress.checking = false;
                        progress.checks = checks;
                    }),
                    command = commands.recv() => {
                        progress.send_modify(|progress| progress.checking = false);
                        match command {
                            Some(SetupCommand::Cancel) => {}
                            Some(command) => next = Some(command),
                            None => return,
                        }
                    }
                    _ = stop.wait_for(|stop| *stop) => return,
                }
            }
            SetupCommand::Install {
                generation,
                runtimes,
            } => {
                if progress.borrow().generation == generation
                    && install(&home, &runtimes, &mut commands, &mut stop, &progress).await
                        == Ended::Stopped
                {
                    return;
                }
            }
            SetupCommand::Cancel => {}
        }
    }
}

async fn check(home: &HomeLayout) -> Vec<SetupInspection> {
    let (codex, claude, pi, opencode) = tokio::join!(
        setup::inspect(home, LocalSessionRuntime::Codex.as_str()),
        setup::inspect(home, LocalSessionRuntime::Claude.as_str()),
        setup::inspect(home, LocalSessionRuntime::Pi.as_str()),
        setup::inspect(home, LocalSessionRuntime::Opencode.as_str()),
    );
    vec![codex, claude, pi, opencode]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ended {
    Finished,
    Cancelled,
    Stopped,
}

// Dropping an unfinished `enable` future ends its provider command, whose
// process tree the runtime capture owns.
async fn install(
    home: &HomeLayout,
    runtimes: &[LocalSessionRuntime],
    commands: &mut mpsc::UnboundedReceiver<SetupCommand>,
    stop: &mut watch::Receiver<bool>,
    progress: &watch::Sender<SetupProgress>,
) -> Ended {
    progress.send_modify(|progress| {
        progress.installs = runtimes
            .iter()
            .map(|runtime| (*runtime, InstallState::Waiting))
            .collect();
    });
    for (index, runtime) in runtimes.iter().enumerate() {
        set(progress, index, InstallState::Installing);
        let (state, ended) = tokio::select! {
            result = setup::enable(home, runtime.as_str()) => match result {
                Ok(result) => (
                    InstallState::Written {
                        provider_dir: result.provider_dir,
                        warnings: result.warnings,
                    },
                    None,
                ),
                Err(error) => (InstallState::Failed(format!("{error:#}")), None),
            },
            () = cancel_requested(commands) => (InstallState::Interrupted, Some(Ended::Cancelled)),
            _ = stop.wait_for(|stop| *stop) => (InstallState::Interrupted, Some(Ended::Stopped)),
        };
        set(progress, index, state);
        if let Some(ended) = ended {
            for rest in index + 1..runtimes.len() {
                set(progress, rest, InstallState::Skipped);
            }
            return ended;
        }
    }
    Ended::Finished
}

// Only a cancel can arrive while installing, since the flow offers nothing
// else then; an interface that went away cancels too.
async fn cancel_requested(commands: &mut mpsc::UnboundedReceiver<SetupCommand>) {
    while let Some(command) = commands.recv().await {
        if command == SetupCommand::Cancel {
            return;
        }
    }
}

fn set(progress: &watch::Sender<SetupProgress>, index: usize, state: InstallState) {
    progress.send_modify(|progress| {
        if let Some((_, slot)) = progress.installs.get_mut(index) {
            *slot = state;
        }
    });
}
