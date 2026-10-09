#[cfg(all(test, unix))]
mod claude_resume_tests;
mod events;
mod events_migration;
#[cfg(test)]
mod events_migration_tests;
mod managed_reader;
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
pub(crate) use supervisor::registered_running;
pub(crate) use supervisor::{
    cancel_turn, respond_interaction, respond_permission, structured_state,
};

use cccc_core::HomeLayout;
use serde::Serialize;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

pub use supervisor::{
    detach_after_viewer_exit, ensure_viewer, kill_all_requests, running, start, status, stop,
    stop_all, stop_group, submit_batch, supports,
};

pub(super) fn uses_managed_session(actor: &cccc_contracts::Actor) -> bool {
    supervisor::uses_managed_session(actor)
}

/// Delivery follows the current owner; saved settings apply on the next launch.
/// A temporary Claude trust terminal belongs to recovery, not native task input.
pub(super) fn uses_managed_delivery(group_id: &str, actor: &cccc_contracts::Actor) -> bool {
    registered_running(group_id, &actor.id).is_some()
        || workspace_trust_recovery::registered(&(group_id.to_owned(), actor.id.clone()))
            .unwrap_or(true)
        || (supports(actor)
            && !cccc_runtime::status(group_id, &actor.id).is_ok_and(|status| status.running))
}

pub(super) fn uses_managed_provider_cli(actor: &cccc_contracts::Actor) -> bool {
    provider_cli::uses_managed_provider_cli(actor)
}

#[derive(Debug, Clone, Serialize)]
pub struct HeadlessStatus {
    pub status: String,
    pub task_id: Option<String>,
    pub updated_at: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSubmission {
    Deferred,
    Accepted,
    Unconfirmed,
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
}

struct Session {
    home: HomeLayout,
    group_id: String,
    actor_id: String,
    managed: std::sync::Arc<super::codex_voice_analyst::AnalystSession>,
    has_terminal: AtomicBool,
    viewer: Mutex<Option<ViewerLaunch>>,
    status: Mutex<HeadlessStatus>,
    stopped: AtomicBool,
    stop_lock: Mutex<()>,
    startup_prompt: Mutex<Option<String>>,
    active_turn: Mutex<Option<ActiveTurn>>,
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

fn block_on_managed<F>(future: F) -> F::Output
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
