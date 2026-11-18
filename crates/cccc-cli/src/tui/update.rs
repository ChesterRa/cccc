//! The work behind `/update`: one check each time the user asks, then, once
//! confirmed, the host's attended update to a release or restart into a
//! rebuilt program. It runs as a Tokio task fed by the interface thread, and
//! stops when the host stops it.

use crate::update_checker::UpdateEntry;
use cccc_web::{UpdateCheck, UpdateRequest, UpdateSource};
use tokio::sync::{mpsc, watch};

/// Each `/update` visit has its own generation, so a result left over from an
/// earlier visit is never read as this one's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum UpdateCommand {
    Check {
        generation: u64,
    },
    Start {
        generation: u64,
        request: UpdateRequest,
        /// The version it starts, as the screen shows it.
        version: String,
    },
    /// Ends a running check, or gives up an update still being prepared.
    Cancel,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct UpdateProgress {
    pub(super) generation: u64,
    pub(super) checking: bool,
    pub(super) check: Option<UpdateCheck>,
    /// The version being prepared, until the host takes it over or it ends.
    pub(super) preparing: Option<String>,
    /// The confirmed start restarts into a rebuilt program, not a release.
    pub(super) local: bool,
    /// Why the confirmed update did not start. Nothing was stopped.
    pub(super) rejected: Option<String>,
    /// The confirmed update was given up while it was being prepared.
    pub(super) given_up: bool,
    /// The host took the update over and is replacing this process.
    pub(super) handed_over: bool,
}

/// Serve the interface's update commands until `stop` turns true or the
/// interface goes away. Returning drops an update still being prepared,
/// which gives it up.
pub(super) async fn run(
    entry: UpdateEntry,
    mut commands: mpsc::UnboundedReceiver<UpdateCommand>,
    progress: watch::Sender<UpdateProgress>,
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
            UpdateCommand::Check { generation } => {
                progress.send_replace(UpdateProgress {
                    generation,
                    checking: true,
                    ..UpdateProgress::default()
                });
                tokio::select! {
                    check = entry.check() => progress.send_modify(|progress| {
                        progress.checking = false;
                        progress.check = Some(check);
                    }),
                    command = commands.recv() => {
                        progress.send_modify(|progress| progress.checking = false);
                        match command {
                            Some(UpdateCommand::Cancel) => {}
                            Some(command) => next = Some(command),
                            None => return,
                        }
                    }
                    _ = stop.wait_for(|stop| *stop) => return,
                }
            }
            UpdateCommand::Start {
                generation,
                request,
                version,
            } => {
                if progress.borrow().generation != generation {
                    continue;
                }
                let local = matches!(request.source, UpdateSource::Local { .. });
                progress.send_modify(|progress| {
                    progress.preparing = Some(version.clone());
                    progress.local = local;
                    progress.rejected = None;
                    progress.given_up = false;
                });
                tokio::select! {
                    result = entry.start(request) => progress.send_modify(|progress| {
                        progress.preparing = None;
                        match result {
                            Ok(_) => progress.handed_over = true,
                            Err(rejection) => progress.rejected = Some(rejection.message),
                        }
                    }),
                    () = cancel_requested(&mut commands) => progress.send_modify(|progress| {
                        progress.preparing = None;
                        progress.given_up = true;
                    }),
                    _ = stop.wait_for(|stop| *stop) => return,
                }
            }
            UpdateCommand::Cancel => {}
        }
    }
}

// Only a cancel can arrive while an update is prepared, since the screen
// offers nothing else then; an interface that went away cancels too.
async fn cancel_requested(commands: &mut mpsc::UnboundedReceiver<UpdateCommand>) {
    while let Some(command) = commands.recv().await {
        if command == UpdateCommand::Cancel {
            return;
        }
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
