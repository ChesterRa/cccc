use cccc_core::HomeLayout;
use cccc_core::codex_voice_settings::ResolvedAgentRuntime;
use cccc_daemon::experimental_codex_voice::{
    AnalystLifecycleEvent, CodexVoiceAnalyst, CodexVoiceCall, LaunchConfig, RealtimeCallConfig,
    create_realtime_answer,
};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::codex_voice_actor_results::{self, ObservedActorResult};
use crate::ledger_event_hub::LedgerEventHub;

mod active_session;
mod analyst_events;
mod analyst_runtime;
mod analyst_settings;
mod analyst_terminal;
mod persistence;
mod sessions_lifecycle;
mod sessions_start;
#[cfg(test)]
mod tests;

const CONNECTION_UNATTACHED: u8 = 0;
const CONNECTION_ATTACHED: u8 = 1;
const CONNECTION_CLOSING: u8 = 2;

#[derive(Debug, Clone)]
pub(crate) struct SessionInfo {
    pub generation: String,
    pub analyst_generation: String,
    pub voice: String,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AnalystInfo {
    pub generation: String,
    pub tui_ready: bool,
    pub phase: String,
    pub last_result: String,
    pub warning: String,
}

#[derive(Debug, Clone)]
pub(crate) struct VoiceState {
    pub call: Option<SessionInfo>,
    pub analyst: Option<AnalystInfo>,
}

pub(crate) struct StartedSession {
    pub session: Arc<ActiveSession>,
    pub answer_sdp: String,
    pub newly_created: bool,
}

pub(crate) enum StartOutcome {
    Started(StartedSession),
    Busy(SessionInfo),
}

#[derive(Debug, Default)]
struct AnalystSnapshot {
    phase: String,
    last_result: String,
    warning: String,
}

pub(crate) struct AnalystRuntime {
    workdir: PathBuf,
    analyst: Arc<CodexVoiceAnalyst>,
    launch_runtime: ResolvedAgentRuntime,
    terminal_gate: Mutex<()>,
    snapshot: StdMutex<AnalystSnapshot>,
    monitor: StdMutex<Option<JoinHandle<()>>>,
    call_generation: StdMutex<Option<String>>,
    tracked_work: StdMutex<HashSet<String>>,
    pending_results: StdMutex<VecDeque<(String, ObservedActorResult)>>,
    actor_result_gate: Mutex<()>,
}

pub(crate) struct ActiveSession {
    call: Arc<CodexVoiceCall>,
    analyst: Arc<AnalystRuntime>,
    client_session_id: String,
    offer_digest: [u8; 32],
    answer_sdp: String,
    voice: String,
    connection_state: AtomicU8,
}

pub(crate) struct SessionAttachment {
    session: Arc<ActiveSession>,
}

#[derive(Default)]
struct ManagedState {
    active: Option<Arc<ActiveSession>>,
    analyst: Option<Arc<AnalystRuntime>>,
}

pub(crate) struct AnalystSettingsOutcome {
    pub analyst: Option<AnalystInfo>,
    pub restarted: bool,
    pub started_new_session: bool,
    pub discarded_work: bool,
}

pub(crate) struct CodexVoiceSessions {
    state: Mutex<ManagedState>,
    ledger_events: Option<LedgerEventHub>,
}
