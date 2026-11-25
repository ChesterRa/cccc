#[cfg(all(test, unix))]
mod claude_resume_tests;
mod confirmation_pause;
mod confirmation_terminal;
mod events;
mod events_migration;
#[cfg(test)]
mod events_migration_tests;
mod hermes_setup;
mod managed_reader;
mod notice_gate;
mod output;
mod provider_cli;
mod session;
mod supervisor;
#[cfg(test)]
mod supervisor_managed_tests;
mod workspace_trust;
mod workspace_trust_recovery;

#[cfg(test)]
pub(crate) use managed_reader::verify_claude_reader_release;

pub(crate) use events::{
    append as append_event, append_with_dedupe as append_event_with_dedupe,
    contains_dedupe as contains_event_dedupe,
};
pub(crate) use notice_gate::held_batch;
pub(crate) use supervisor::submit_prepared;
pub(crate) use supervisor::{
    cancel_turn, respond_interaction, respond_permission, structured_state,
};
pub(crate) use supervisor::{
    pauses_after_confirmation, registered_generation, registered_running, registered_workspace,
};

use cccc_contracts::event::DeliveryEvidence;
use cccc_core::HomeLayout;
use serde::Serialize;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

#[cfg(test)]
pub use supervisor::submit_batch;
pub(crate) use supervisor::submit_batch_marked;
pub use supervisor::{
    detach_after_viewer_exit, ensure_viewer, kill_all_requests, ready_for_mail_notice, running,
    start, status, stop, stop_all, stop_group, supports,
};

pub(super) fn uses_managed_session(actor: &cccc_contracts::Actor) -> bool {
    supervisor::uses_managed_session(actor)
}

pub(crate) fn waiting_for_workspace_trust(group_id: &str, actor_id: &str) -> std::io::Result<bool> {
    #[cfg(unix)]
    if let Some(snapshot) = crate::session_owner::snapshot(group_id, actor_id)? {
        return Ok(snapshot.waiting_for_workspace_trust);
    }
    workspace_trust_recovery::registered(&(group_id.to_owned(), actor_id.to_owned()))
}

pub(crate) fn confirmation_terminal_owns(group_id: &str, actor_id: &str) -> std::io::Result<bool> {
    #[cfg(unix)]
    if let Some(snapshot) = crate::session_owner::snapshot(group_id, actor_id)? {
        return Ok(snapshot.confirmation_terminal);
    }
    confirmation_terminal::owns(group_id, actor_id)
}

pub(crate) fn confirmation_pending_active(group_id: &str, actor_id: &str) -> std::io::Result<bool> {
    #[cfg(unix)]
    if let Some(snapshot) = crate::session_owner::snapshot(group_id, actor_id)? {
        return Ok(snapshot.confirmation_pending);
    }
    workspace_trust_recovery::active(&(group_id.to_owned(), actor_id.to_owned()))
}

/// A protected confirmation PTY with no active watcher cannot complete its recovery.
pub(super) fn orphaned_confirmation(group_id: &str, actor_id: &str) -> std::io::Result<bool> {
    Ok(!confirmation_pending_active(group_id, actor_id)?
        && confirmation_terminal::owns(group_id, actor_id)?)
}

pub(crate) fn pause_confirmation(group_id: &str, actor_id: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    if crate::session_owner::control(
        group_id,
        actor_id,
        None,
        crate::session_owner::Control::PauseConfirmation,
    )?
    .is_some()
    {
        return Ok(());
    }
    confirmation_pause::pause((group_id.to_owned(), actor_id.to_owned()))
}

pub(crate) fn confirmation_paused(group_id: &str, actor_id: &str) -> std::io::Result<bool> {
    #[cfg(unix)]
    if let Some(snapshot) = crate::session_owner::snapshot(group_id, actor_id)? {
        return Ok(snapshot.confirmation_paused);
    }
    confirmation_pause::is_paused(&(group_id.to_owned(), actor_id.to_owned()))
}

/// Read where the pause lives: a session owner keeps its members' pauses, and
/// a start must be refused there before it retires the owner's registration.
pub(super) fn ensure_confirmation_allowed(group_id: &str, actor_id: &str) -> std::io::Result<()> {
    if confirmation_paused(group_id, actor_id)? {
        return Err(confirmation_pause::paused_error());
    }
    Ok(())
}

pub(super) fn is_confirmation_paused_error(error: &std::io::Error) -> bool {
    confirmation_pause::is_paused_error(error)
}

pub(crate) fn retry_confirmation(group_id: &str, actor_id: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    if crate::session_owner::control(
        group_id,
        actor_id,
        None,
        crate::session_owner::Control::RetryConfirmation,
    )?
    .is_some()
    {
        return Ok(());
    }
    let key = (group_id.to_owned(), actor_id.to_owned());
    workspace_trust_recovery::cancel(&key)?;
    cccc_runtime::reset_exited_startup_input(group_id, actor_id)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    confirmation_pause::retry(&key)
}

/// A session owner retiring a member's registration: nothing outside the owner
/// could observe or clear its confirmation state afterwards. Cancelling first
/// keeps a late exit of the retired setup or prompt from pausing again.
pub(crate) fn forget_confirmation(group_id: &str, actor_id: &str) -> std::io::Result<()> {
    let key = (group_id.to_owned(), actor_id.to_owned());
    workspace_trust_recovery::cancel(&key)?;
    confirmation_pause::retry(&key)
}

/// Called after Group deletion commits; its runtime teardown already cancelled old recoveries.
pub(crate) fn clear_group_confirmation_pauses(group_id: &str) -> std::io::Result<()> {
    confirmation_pause::clear_group(group_id)
}

/// Delivery follows the current owner; saved settings apply on the next launch.
/// A temporary Claude trust terminal belongs to recovery, not native task input.
pub(super) fn uses_managed_delivery(group_id: &str, actor: &cccc_contracts::Actor) -> bool {
    registered_running(group_id, &actor.id).is_some()
        // The recovery is registered before its PTY is published, and the terminal
        // marker is installed before recovery can retire. Read in that handoff order.
        || workspace_trust_recovery::registered(&(group_id.to_owned(), actor.id.clone()))
            .unwrap_or(true)
        || confirmation_terminal::owns(group_id, &actor.id).unwrap_or(true)
        || (supports(actor)
            && !cccc_runtime::status(group_id, &actor.id).is_ok_and(|status| status.running))
}

pub(super) fn uses_managed_provider_cli(actor: &cccc_contracts::Actor) -> bool {
    provider_cli::uses_managed_provider_cli(actor)
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct HeadlessStatus {
    pub status: String,
    pub task_id: Option<String>,
    pub updated_at: String,
    pub pid: Option<u32>,
    /// Provider-reported cause for a non-nominal status (a failed turn).
    /// None for every state whose meaning is already carried by `status`.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSubmission {
    Deferred,
    Accepted(Option<DeliveryEvidence>),
    Unconfirmed,
    /// Early Mail notices the Actor is no longer ready for. Nothing was sent;
    /// the unread tick re-issues them once the Actor is idle and the Mail is
    /// still unread, so they are withdrawn rather than retried.
    Withheld,
}

impl BatchSubmission {
    fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted(_))
    }

    #[cfg(unix)]
    pub(crate) fn owner_result(self, started: bool) -> serde_json::Value {
        use serde_json::json;
        let outcome = match self {
            Self::Deferred => "Deferred",
            Self::Accepted(_) => "Accepted",
            Self::Unconfirmed => "Unconfirmed",
            Self::Withheld => "Withheld",
        };
        let mut result = json!({"backend":"managed","outcome":outcome,"started":started});
        if let Self::Accepted(Some(evidence)) = self {
            result["delivery_evidence"] = json!(evidence);
        }
        result
    }

    #[cfg(unix)]
    pub(crate) fn from_owner_result(result: &serde_json::Value) -> Option<Self> {
        Some(match result.get("outcome")?.as_str()? {
            "Deferred" => Self::Deferred,
            "Accepted" => Self::Accepted(DeliveryEvidence::from_json(&result["delivery_evidence"])),
            "Unconfirmed" => Self::Unconfirmed,
            "Withheld" => Self::Withheld,
            _ => return None,
        })
    }
}

#[derive(Debug)]
struct ActiveTurn {
    turn_id: String,
}

/// Everything needed to re-open the viewer attachment (`claude attach` or
/// the remote TUI) after a detached attach exit.
#[derive(Debug, Clone)]
struct ViewerLaunch {
    command: Vec<String>,
    env: std::collections::BTreeMap<String, String>,
    cwd: std::path::PathBuf,
    submit: cccc_contracts::ActorSubmit,
    /// The last Kilo viewer whose first input the bound session recorded.
    confirmed_input: Option<(u32, String)>,
}

struct Session {
    home: HomeLayout,
    group_id: String,
    actor_id: String,
    // Pure headless sessions may never have a viewer. The provider's launch
    // workspace belongs to this owner, independently of terminal attachment.
    workspace: super::actor_runtime::LaunchWorkspace,
    managed: std::sync::Arc<super::codex_voice_analyst::AnalystSession>,
    has_terminal: AtomicBool,
    viewer: Mutex<Option<ViewerLaunch>>,
    status: Mutex<HeadlessStatus>,
    stopped: AtomicBool,
    stop_lock: Mutex<()>,
    startup_prompt: Mutex<Option<String>>,
    active_turn: Mutex<Option<ActiveTurn>>,
    /// Values the activity journal redacts. They come from the environment this
    /// launch delivered, so a later settings change cannot unmask what the
    /// running provider still holds.
    journal_private: Vec<String>,
}

fn poisoned() -> std::io::Error {
    std::io::Error::other("headless supervisor lock poisoned")
}

fn managed_runtime() -> &'static tokio::runtime::Runtime {
    super::codex_voice_analyst::managed_runtime()
}

fn run_managed_launch<F, T>(future: F) -> std::io::Result<T>
where
    F: Future<Output = std::io::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    // Runtime::block_on polls its root future on the caller. Grok ACP binds
    // itself to the spawning OS thread on Linux, so a restore/request thread
    // must only wait here; the daemon-lifetime workers must perform the launch.
    let task = managed_runtime().spawn(future);
    block_on_managed(task).map_err(|error| {
        std::io::Error::other(format!("managed Agent launch task failed: {error}"))
    })?
}

pub(super) fn block_on_managed<F>(future: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::scope(|scope| {
            match scope
                .spawn(move || managed_runtime().block_on(future))
                .join()
            {
                Ok(output) => output,
                Err(panic) => std::panic::resume_unwind(panic),
            }
        });
    }
    managed_runtime().block_on(future)
}
