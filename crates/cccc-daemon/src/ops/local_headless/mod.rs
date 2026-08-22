mod events;
mod events_migration;
#[cfg(test)]
mod events_migration_tests;
mod output;
mod protocol;
mod provider_cli;
mod session;
mod supervisor;

pub(crate) use events::{
    append as append_event, append_with_dedupe as append_event_with_dedupe,
    contains_dedupe as contains_event_dedupe,
};

use cccc_contracts::ActorRuntime;
use cccc_core::HomeLayout;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Condvar, Mutex};

pub use supervisor::{running, start, status, stop, stop_all, stop_group, submit, supports};

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
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    status: Mutex<HeadlessStatus>,
    stopped: AtomicBool,
    next_request_id: AtomicU64,
    pending: Mutex<HashMap<u64, SyncSender<Value>>>,
    thread_id: Mutex<String>,
    resumed_provider_session_id: Mutex<String>,
    active_turn: Mutex<Option<ActiveTurn>>,
    completion: (Mutex<u64>, Condvar),
    turns: SyncSender<Turn>,
}

fn turn_channel() -> (SyncSender<Turn>, std::sync::mpsc::Receiver<Turn>) {
    sync_channel(256)
}

fn poisoned() -> std::io::Error {
    std::io::Error::other("headless supervisor lock poisoned")
}
