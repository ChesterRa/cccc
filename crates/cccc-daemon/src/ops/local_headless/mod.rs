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

#[cfg(test)]
pub(crate) use managed_reader::verify_claude_reader_release;

pub(crate) use events::{
    append as append_event, append_with_dedupe as append_event_with_dedupe,
    contains_dedupe as contains_event_dedupe,
};

use cccc_core::HomeLayout;
use serde::Serialize;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
pub use supervisor::submit;
pub use supervisor::{running, start, status, stop, stop_all, stop_group, submit_batch, supports};

pub(super) fn uses_managed_session(actor: &cccc_contracts::Actor) -> bool {
    supervisor::uses_managed_session(actor)
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

#[derive(Debug)]
struct ActiveTurn {
    turn_id: String,
}

struct Session {
    home: HomeLayout,
    group_id: String,
    actor_id: String,
    managed: std::sync::Arc<super::codex_voice_analyst::AnalystSession>,
    has_terminal: AtomicBool,
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
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("cccc-managed-agent")
            .enable_all()
            .build()
            .expect("build shared managed Agent runtime")
    })
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
