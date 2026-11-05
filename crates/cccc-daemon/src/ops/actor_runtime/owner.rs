//! What runs for an Actor now. Saved settings describe the next launch; a
//! delivery or a pull that follows them could reach the wrong session.

use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};
use cccc_runtime::{ActorLaunch, RuntimeError, SessionStatus};
use std::io;

use crate::dispatch::OpError;
use crate::ops::{deepseek_runtime, local_headless};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerState {
    Running,
    /// Failed or in trust recovery: not taking work, and not gone either.
    Holding,
    /// Its process exited; only the terminal is kept.
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerIdentity {
    DeepSeek {
        generation: String,
    },
    /// A trust or confirmation terminal runs before the session has a generation.
    Managed {
        generation: Option<String>,
    },
    Pty {
        pid: Option<u32>,
        started_at: String,
        launch: ActorLaunch,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisteredOwner {
    pub(crate) identity: OwnerIdentity,
    pub(crate) state: OwnerState,
}

impl RegisteredOwner {
    fn new(identity: OwnerIdentity, running: bool, stopped: OwnerState) -> Self {
        let state = if running {
            OwnerState::Running
        } else {
            stopped
        };
        Self { identity, state }
    }

    fn of_pty(status: &SessionStatus, launch: ActorLaunch) -> Self {
        Self::new(
            OwnerIdentity::Pty {
                pid: status.pid,
                started_at: status.started_at.clone(),
                launch,
            },
            status.running,
            OwnerState::Exited,
        )
    }
}

/// The registered owner, or `None` when only saved settings apply. A failed
/// query is an error: guessing here would hand work to the wrong session.
pub(crate) fn registered_owner(
    group_id: &str,
    actor_id: &str,
) -> io::Result<Option<RegisteredOwner>> {
    // Every stop removes or refuses a DeepSeek entry before another owner starts,
    // so a registered one is the only owner.
    if let Some((generation, running)) = deepseek_runtime::registered_generation(group_id, actor_id)
    {
        return Ok(Some(RegisteredOwner::new(
            OwnerIdentity::DeepSeek { generation },
            running,
            OwnerState::Holding,
        )));
    }
    if let Some((generation, running)) = local_headless::registered_generation(group_id, actor_id) {
        return Ok(Some(RegisteredOwner::new(
            OwnerIdentity::Managed {
                generation: Some(generation),
            },
            running,
            OwnerState::Holding,
        )));
    }
    // The recovery is registered before its terminal is published, and the
    // terminal's marker before recovery retires. Read in that handoff order.
    if local_headless::waiting_for_workspace_trust(group_id, actor_id)?
        || local_headless::confirmation_terminal_owns(group_id, actor_id)?
    {
        return Ok(Some(RegisteredOwner::new(
            OwnerIdentity::Managed { generation: None },
            false,
            OwnerState::Holding,
        )));
    }
    match cccc_runtime::status(group_id, actor_id) {
        Ok(status) => Ok(status
            .launch
            .map(|launch| RegisteredOwner::of_pty(&status, launch))),
        Err(RuntimeError::NotFound(_, _)) => Ok(None),
        Err(error) => Err(io::Error::other(error)),
    }
}

/// Who takes deliveries now. An exited terminal is only kept for reading.
pub(crate) fn delivery_owner(
    group_id: &str,
    actor_id: &str,
) -> io::Result<Option<RegisteredOwner>> {
    Ok(registered_owner(group_id, actor_id)?.filter(|owner| owner.state != OwnerState::Exited))
}

/// Browser work in progress ends before the Actor's route changes. This answers
/// busy rather than waits: a holder may be calling the daemon right now. Dropping
/// the file releases it.
pub(crate) fn hold_browser_work(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Result<std::fs::File, OpError> {
    let lock = cccc_core::web_model_connectors::browser_work_lock(home, group_id, actor_id);
    cccc_core::fs::try_exclusive_lock(&lock)
        .map_err(OpError::io)?
        .ok_or_else(|| {
            OpError::new(
                "actor_browser_busy",
                "This member is delivering to its browser. Try again in a moment.",
            )
        })
}

pub(crate) fn hold_group_browser_work(
    home: &HomeLayout,
    group: &GroupDoc,
) -> Result<Vec<std::fs::File>, OpError> {
    group
        .actors
        .iter()
        .map(|actor| hold_browser_work(home, &group.group_id, &actor.id))
        .collect()
}

/// The Actor as its terminal launched. Saved settings apply from the next launch.
pub(crate) fn as_launched(actor: &Actor, launch: ActorLaunch) -> Actor {
    let mut launched = actor.clone();
    launched.runtime = launch.runtime;
    launched.submit = launch.submit;
    launched
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::{ActorRuntime, ActorSubmit, RunnerKind};
    use cccc_runtime::LaunchSpec;

    fn spec(group_id: &str, actor_id: &str, cwd: &std::path::Path) -> LaunchSpec {
        LaunchSpec {
            group_id: group_id.into(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), "exec sleep 30".into()],
            cwd: cwd.into(),
            env: Default::default(),
            cols: 80,
            rows: 24,
        }
    }

    #[test]
    fn only_a_launched_terminal_is_an_owner() {
        let temp = tempfile::tempdir().expect("fixture");
        let group = "g_registered_owner";
        assert_eq!(registered_owner(group, "none").expect("query"), None);

        // A viewer or other shared terminal runs nobody's launch.
        cccc_runtime::start(spec(group, "viewer", temp.path())).expect("terminal");
        assert_eq!(registered_owner(group, "viewer").expect("query"), None);
        cccc_runtime::stop(group, "viewer").expect("stop");

        let launch = ActorLaunch {
            runtime: ActorRuntime::Custom,
            submit: ActorSubmit::Newline,
        };
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let history =
            super::super::terminal_history::config(&home, group, "member").expect("history");
        let status = cccc_runtime::start_actor_with_history(
            spec(group, "member", temp.path()),
            history,
            launch,
        )
        .expect("primary terminal");
        let owner = registered_owner(group, "member")
            .expect("query")
            .expect("owner");
        assert_eq!(owner.state, OwnerState::Running);
        assert_eq!(
            owner.identity,
            OwnerIdentity::Pty {
                pid: status.pid,
                started_at: status.started_at,
                launch,
            }
        );
        cccc_runtime::stop(group, "member").expect("stop");
        assert_eq!(registered_owner(group, "member").expect("query"), None);
    }
}
