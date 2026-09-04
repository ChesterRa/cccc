use super::codex_voice_analyst::{AnalystEvent, AnalystSession, ElicitationAction, TurnReceipt};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{Mutex as AsyncMutex, broadcast};
use tokio::task::JoinHandle;

mod events;
#[cfg(test)]
mod tests;
mod turn_wait;
mod turns;

const EVENT_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalystTurnOrigin {
    Voice,
    Terminal,
    ActorResult { speakable: bool },
}

impl AnalystTurnOrigin {
    pub fn speakable(self) -> bool {
        matches!(self, Self::Voice | Self::ActorResult { speakable: true })
    }

    pub fn is_actor_result(self) -> bool {
        matches!(self, Self::ActorResult { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedWork {
    pub group_id: String,
    pub task_id: String,
    pub source_event_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone)]
pub enum AnalystLifecycleEvent {
    Started {
        receipt: TurnReceipt,
        origin: AnalystTurnOrigin,
    },
    Associated {
        receipt: TurnReceipt,
        origin: AnalystTurnOrigin,
    },
    Progress {
        turn_id: String,
        text: String,
        speakable: bool,
    },
    Completed {
        turn_id: String,
        delegation_id: String,
        status: String,
        result: String,
        speakable: bool,
    },
    TrackedWork(TrackedWork),
    NeedsAttention {
        code: &'static str,
    },
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceDelegationAdmission {
    Turn(TurnReceipt),
    NativeInput { delegation_id: String, text: String },
    NativeInputPending,
}

#[derive(Debug)]
struct ActiveTurn {
    turn_id: String,
    latest_delegation_id: String,
    origin: AnalystTurnOrigin,
    cancelling: bool,
    deltas: String,
    completed_text: String,
    result_overflowed: bool,
}

#[derive(Debug, Clone)]
struct PendingStart {
    delegation_id: String,
    origin: AnalystTurnOrigin,
}

#[derive(Debug, Default)]
struct LifecycleState {
    active: Option<ActiveTurn>,
    pending: Option<PendingStart>,
    settled_pending: Option<TurnReceipt>,
    native_pending: VecDeque<PendingStart>,
    delegations: HashMap<String, TurnReceipt>,
    invalidated: bool,
}

pub(crate) struct AnalystLifecycle {
    session: Arc<AnalystSession>,
    state: AsyncMutex<LifecycleState>,
    events: broadcast::Sender<AnalystLifecycleEvent>,
    monitor: Mutex<Option<JoinHandle<()>>>,
}

impl AnalystLifecycle {
    pub(crate) fn start(session: Arc<AnalystSession>) -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let lifecycle = Arc::new(Self {
            session,
            state: AsyncMutex::new(LifecycleState::default()),
            events,
            monitor: Mutex::new(None),
        });
        let weak = Arc::downgrade(&lifecycle);
        let mut source = lifecycle.session.subscribe();
        let task = tokio::spawn(async move {
            loop {
                match source.recv().await {
                    Ok(event) => {
                        let disconnected = event.message["method"]
                            == super::codex_voice_analyst::MANAGED_AGENT_DISCONNECTED_METHOD;
                        if disconnected && event.message["params"]["expected"] == true {
                            break;
                        }
                        let Some(lifecycle) = Weak::upgrade(&weak) else {
                            break;
                        };
                        lifecycle.handle(event).await;
                        if disconnected {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped,
                            "Voice Analyst lifecycle reader fell behind; invalidating the unreplayable session"
                        );
                        if let Some(lifecycle) = Weak::upgrade(&weak) {
                            lifecycle.invalidate().await;
                            if let Err(error) =
                                lifecycle.session.stop(lifecycle.session.generation()).await
                            {
                                tracing::warn!(%error, "failed to stop invalid Voice Analyst session");
                            }
                        }
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        if let Some(lifecycle) = Weak::upgrade(&weak) {
                            lifecycle.invalidate().await;
                        }
                        break;
                    }
                }
            }
        });
        *lifecycle
            .monitor
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(task);
        lifecycle
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<AnalystLifecycleEvent> {
        self.events.subscribe()
    }

    async fn invalidate(&self) {
        let mut state = self.state.lock().await;
        state.active = None;
        state.pending = None;
        state.settled_pending = None;
        state.native_pending.clear();
        state.invalidated = true;
        let _ = self.events.send(AnalystLifecycleEvent::Disconnected);
    }
}

impl Drop for AnalystLifecycle {
    fn drop(&mut self) {
        if let Ok(mut monitor) = self.monitor.lock()
            && let Some(task) = monitor.take()
        {
            task.abort();
        }
    }
}
