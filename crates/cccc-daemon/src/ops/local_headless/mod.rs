mod events;
mod events_migration;
#[cfg(test)]
mod events_migration_tests;
mod managed_reader;
mod output;
mod protocol;
mod provider_cli;
mod session;
mod supervisor;
#[cfg(test)]
mod supervisor_managed_tests;

pub(crate) use events::{
    append as append_event, append_with_dedupe as append_event_with_dedupe,
    contains_dedupe as contains_event_dedupe,
};

use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Condvar, Mutex, OnceLock};

pub use supervisor::{
    restore, running, start, status, stop, stop_all, stop_group, submit, supports,
};

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
struct Turn {
    text: String,
    event_id: String,
    control_kind: String,
}

#[derive(Debug)]
struct ActiveTurn {
    event_id: String,
    turn_id: String,
    control_kind: String,
    external: bool,
    output_state: TurnOutputState,
    pending_messages: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnOutputState {
    Buffering,
    Draining,
    Announced,
}

struct Session {
    home: HomeLayout,
    group_id: String,
    actor_id: String,
    runtime: ActorRuntime,
    transport: SessionTransport,
    status: Mutex<HeadlessStatus>,
    stopped: AtomicBool,
    next_request_id: AtomicU64,
    pending: Mutex<HashMap<u64, SyncSender<Value>>>,
    thread_id: Mutex<String>,
    resumed_provider_session_id: Mutex<String>,
    startup_prompt: Mutex<Option<String>>,
    active_turn: Mutex<Option<ActiveTurn>>,
    completion: (Mutex<u64>, Condvar),
    turns: SyncSender<Turn>,
}

impl Session {
    fn is_managed(&self) -> bool {
        matches!(&self.transport, SessionTransport::ManagedAgent { .. })
    }

    fn uses_structured_turn_protocol(&self) -> bool {
        self.runtime == ActorRuntime::Codex || self.is_managed()
    }
}

enum SessionTransport {
    Process {
        child: Mutex<Child>,
        stdin: Mutex<ChildStdin>,
    },
    ManagedAgent {
        session: std::sync::Arc<super::codex_voice_analyst::AnalystSession>,
        has_terminal: AtomicBool,
    },
}

fn turn_channel() -> (SyncSender<Turn>, std::sync::mpsc::Receiver<Turn>) {
    sync_channel(256)
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
