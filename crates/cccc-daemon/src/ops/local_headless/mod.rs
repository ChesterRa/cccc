mod events;
mod output;
mod protocol;
mod session;
mod supervisor;

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
    event_ts: String,
    control: bool,
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
    active_event_id: Mutex<String>,
    completion: (Mutex<u64>, Condvar),
    turns: SyncSender<Turn>,
}

fn turn_channel() -> (SyncSender<Turn>, std::sync::mpsc::Receiver<Turn>) {
    sync_channel(256)
}

fn poisoned() -> std::io::Error {
    std::io::Error::other("headless supervisor lock poisoned")
}
