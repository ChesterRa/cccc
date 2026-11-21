use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const MAX_QUEUED_INPUTS: usize = 32;

/// The source worker owns this deadline while it waits to submit the Group
/// request. Its owner can observe the private inbox without submitting work.
#[derive(Debug, Clone)]
pub struct GroupWaitDeadline(Arc<ObservedDeadline>);

#[derive(Debug)]
pub(super) struct ObservedDeadline {
    at: tokio::time::Instant,
    expired_while_blocked: AtomicBool,
}

impl GroupWaitDeadline {
    pub fn new(at: tokio::time::Instant) -> Self {
        Self(Arc::new(ObservedDeadline {
            at,
            expired_while_blocked: AtomicBool::new(false),
        }))
    }

    fn initial_wait(&self) -> GroupWait {
        if self.0.expired_while_blocked.load(Ordering::Acquire) {
            GroupWait::AwaitingDurableExpiry
        } else {
            GroupWait::Until(self.0.at)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GroupWait {
    Until(tokio::time::Instant),
    AwaitingDurableExpiry,
}

#[derive(Debug)]
pub(super) struct QueuedInput {
    pub delegation_id: String,
    pub text: String,
    pub origin: AnalystTurnOrigin,
    wait: Option<GroupWait>,
}

impl QueuedInput {
    pub(super) fn new(id: &str, text: &str, origin: AnalystTurnOrigin) -> Self {
        Self {
            delegation_id: id.into(),
            text: text.into(),
            origin,
            wait: None,
        }
    }

    pub(super) fn wait_expired(&self) -> bool {
        self.wait == Some(GroupWait::AwaitingDurableExpiry)
    }
}

/// Private Chat can share local private or native terminal work, up to the
/// adapter's bounded inbox. All other origins retain a separate turn.
pub(super) fn can_admit_private(state: &LifecycleState) -> bool {
    state.pending.is_none()
        && state.native_pending.len() < MAX_QUEUED_INPUTS
        && state
            .native_pending
            .iter()
            .all(|input| input.origin == AnalystTurnOrigin::ChetText)
        && state.active.as_ref().is_none_or(|turn| {
            matches!(
                turn.origin,
                AnalystTurnOrigin::Terminal | AnalystTurnOrigin::ChetText
            )
        })
}

pub(super) fn can_dispatch_front(state: &LifecycleState) -> bool {
    let Some(front) = state.queue.front() else {
        return false;
    };
    if state.invalidated || front.wait_expired() {
        return false;
    }
    if front.origin == AnalystTurnOrigin::ChetText
        && can_admit_private(state)
        && !state
            .queue
            .iter()
            .any(|item| item.origin == AnalystTurnOrigin::Group)
    {
        return true;
    }
    state.active.is_none()
        && state.pending.is_none()
        && state.native_pending.is_empty()
        && !(state.resumed_inbox_pending && front.origin == AnalystTurnOrigin::Group)
}

pub(super) fn start(lifecycle: &Arc<AnalystLifecycle>) {
    let weak = Arc::downgrade(lifecycle);
    let notify = Arc::clone(&lifecycle.queue_notify);
    let task = tokio::spawn(async move {
        let mut deadline = None;
        loop {
            if let Some(at) = deadline.take() {
                tokio::select! {
                    _ = tokio::time::sleep_until(at) => {},
                    _ = notify.notified() => {},
                }
            } else {
                notify.notified().await;
            }
            let Some(lifecycle) = weak.upgrade() else {
                break;
            };
            loop {
                let mut state = lifecycle.state.lock().await;
                deadline = update_group_waits(&mut state, tokio::time::Instant::now());
                if !can_dispatch_front(&state) {
                    break;
                }
                let input = state.queue.pop_front().expect("eligible queue head");
                lifecycle
                    .queue_len
                    .store(state.queue.len(), std::sync::atomic::Ordering::Release);
                let admission = if input.origin == AnalystTurnOrigin::ChetText {
                    let result = lifecycle
                        .submit_private_inbox(&input.delegation_id, &input.text, &mut state)
                        .await;
                    drop(state);
                    result
                } else {
                    lifecycle
                        .start_new(&input.delegation_id, &input.text, input.origin, state)
                        .await
                        .map(drop)
                };
                match admission {
                    Ok(_) => {}
                    Err(error) if super::turns::is_would_block(&error) => {
                        let mut state = lifecycle.state.lock().await;
                        lifecycle.requeue_after_busy(input, &mut state);
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(%error,"Queued input could not be admitted; no replay");
                        let cancelled = error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::Interrupted);
                        let diagnostic = error.root_cause().to_string();
                        lifecycle.complete_unadmitted_input(
                            &input.delegation_id,
                            input.origin,
                            if cancelled { "cancelled" } else { "failed" },
                            if cancelled { "" } else { &diagnostic },
                            AnalystCompletionSource::HostUnconfirmed,
                        );
                    }
                }
            }
        }
    });
    *lifecycle
        .queue_task
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(task);
}

/// Keep the original deadline across normal host queuing. A private input
/// already ahead in the queue can become an upstream blocker when dispatched.
pub(super) fn update_group_waits(
    state: &mut LifecycleState,
    now: tokio::time::Instant,
) -> Option<tokio::time::Instant> {
    let blocked = state.resumed_inbox_pending
        || state
            .native_pending
            .iter()
            .any(|item| item.origin == AnalystTurnOrigin::ChetText);
    let mut next = None;
    // The submitting worker can be waiting for another Group's durable
    // outcome. Keep what was known at each source deadline even if its private
    // blocker is consumed before that worker reaches admission.
    state.source_waits.retain(|weak| {
        let Some(wait) = weak.upgrade() else {
            return false;
        };
        if wait.at > now {
            return true;
        }
        if blocked {
            wait.expired_while_blocked.store(true, Ordering::Release);
        }
        false
    });
    for input in &mut state.queue {
        if let Some(GroupWait::Until(deadline)) = input.wait
            && blocked
        {
            if deadline <= now {
                input.wait = Some(GroupWait::AwaitingDurableExpiry);
            } else {
                next = Some(next.map_or(deadline, |at: tokio::time::Instant| at.min(deadline)));
            }
        }
    }
    next
}

impl AnalystLifecycle {
    pub(crate) async fn observe_group_wait(&self, wait: &GroupWaitDeadline) {
        if self.session.runtime() != cccc_contracts::ActorRuntime::Opencode {
            return;
        }
        let mut state = self.state.lock().await;
        if !state.invalidated {
            // Only the bounded Web work queue owns strong references.
            state.source_waits.push(Arc::downgrade(&wait.0));
            update_group_waits(&mut state, tokio::time::Instant::now());
        }
    }

    pub(crate) async fn group_wait_expired(&self, id: &str) -> bool {
        self.state
            .lock()
            .await
            .queue
            .iter()
            .any(|input| input.delegation_id == id && input.wait_expired())
    }

    pub(crate) async fn settle_group_wait(&self, id: &str, source: AnalystCompletionSource) {
        let mut state = self.state.lock().await;
        let Some(index) = state
            .queue
            .iter()
            .position(|input| input.delegation_id == id && input.wait_expired())
        else {
            return;
        };
        let input = state.queue.remove(index).expect("position under lock");
        self.queue_len
            .store(state.queue.len(), std::sync::atomic::Ordering::Release);
        let status = if source == AnalystCompletionSource::HostGroupWaitExpired {
            "cancelled"
        } else {
            "unconfirmed"
        };
        self.complete_unadmitted_input(id, input.origin, status, "", source);
        self.queue_notify.notify_one();
    }

    /// Busy is a confirmed rejection. A lifecycle that was invalidated while
    /// admission was in flight cannot consume a requeued input.
    pub(super) fn requeue_after_busy(&self, input: QueuedInput, state: &mut LifecycleState) {
        if state.invalidated {
            self.complete_unadmitted_input(
                &input.delegation_id,
                input.origin,
                "cancelled",
                "",
                AnalystCompletionSource::HostNotStarted,
            );
            return;
        }
        state.queue.push_front(input);
        self.queue_len
            .store(state.queue.len(), std::sync::atomic::Ordering::Release);
    }

    pub(super) fn complete_unadmitted_input(
        &self,
        id: &str,
        origin: AnalystTurnOrigin,
        status: &str,
        error: &str,
        source: AnalystCompletionSource,
    ) {
        // Host outcomes have no provider receipt. One identity per broadcast
        // gives both Web consumers the same distinct persistence key.
        let _ = self.events.send(AnalystLifecycleEvent::Completed {
            source,
            turn_id: format!("host-input-{}", uuid::Uuid::new_v4().simple()),
            delegation_id: id.to_owned(),
            delegation_ids: vec![id.to_owned()],
            status: status.to_owned(),
            error: if status == "failed" {
                super::events::bounded_error(error)
            } else {
                String::new()
            },
            result: String::new(),
            speakable: origin.speakable(),
        });
    }

    pub(super) fn enqueue(
        &self,
        id: &str,
        text: &str,
        origin: AnalystTurnOrigin,
        state: tokio::sync::MutexGuard<'_, LifecycleState>,
    ) -> anyhow::Result<VoiceDelegationAdmission> {
        self.enqueue_until(id, text, origin, state, None)
    }

    pub(super) fn enqueue_until(
        &self,
        id: &str,
        text: &str,
        origin: AnalystTurnOrigin,
        mut state: tokio::sync::MutexGuard<'_, LifecycleState>,
        deadline: Option<&GroupWaitDeadline>,
    ) -> anyhow::Result<VoiceDelegationAdmission> {
        use sha2::Digest;
        let fingerprint: [u8; 32] = sha2::Sha256::digest(text.as_bytes()).into();
        if let Some(existing) = state.acp_inputs.get(id)
            && existing != &(fingerprint, origin)
        {
            anyhow::bail!("ACP input ID was reused with different content")
        }
        if let Some(receipt) = state.delegations.get(id) {
            return Ok(VoiceDelegationAdmission::Turn(receipt.clone()));
        }
        if let Some(index) = state
            .queue
            .iter()
            .position(|input| input.delegation_id == id)
        {
            if state.queue[index].text != text || state.queue[index].origin != origin {
                anyhow::bail!("Queued input ID was reused with different content")
            }
            return Ok(VoiceDelegationAdmission::Queued {
                delegation_id: id.to_owned(),
                position: index + 1,
            });
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.delegation_id == id)
        {
            return Ok(VoiceDelegationAdmission::Queued {
                delegation_id: id.to_owned(),
                position: 0,
            });
        }
        if state.acp_inputs.contains_key(id) {
            anyhow::bail!("ACP input outcome is unresolved or cancelled; it will not be replayed")
        }
        if state.queue.len() >= MAX_QUEUED_INPUTS {
            anyhow::bail!("ACP input queue is full; wait for or cancel the current investigation")
        }
        // A user speaking or typing while a Group input runs should not wait
        // behind additional Group work. Preserve arrival order among user
        // inputs and among all other queued inputs.
        let insert_at = if matches!(
            origin,
            AnalystTurnOrigin::Voice | AnalystTurnOrigin::Terminal | AnalystTurnOrigin::ChetText
        ) {
            state
                .queue
                .iter()
                .position(|input| input.origin == AnalystTurnOrigin::Group)
                .unwrap_or(state.queue.len())
        } else {
            state.queue.len()
        };
        let mut input = QueuedInput::new(id, text, origin);
        input.wait = deadline.map(GroupWaitDeadline::initial_wait);
        state.queue.insert(insert_at, input);
        state
            .acp_inputs
            .insert(id.to_owned(), (fingerprint, origin));
        let position = insert_at + 1;
        self.queue_len
            .store(state.queue.len(), std::sync::atomic::Ordering::Release);
        drop(state);
        self.queue_notify.notify_one();
        let _ = self.events.send(AnalystLifecycleEvent::Queued {
            delegation_id: id.to_owned(),
            position,
        });
        Ok(VoiceDelegationAdmission::Queued {
            delegation_id: id.to_owned(),
            position,
        })
    }

    pub(crate) fn queued_inputs(&self) -> usize {
        self.queue_len.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) async fn input_was_admitted(&self, id: &str) -> bool {
        self.state.lock().await.acp_inputs.contains_key(id)
    }

    pub(crate) async fn admit_terminal(
        &self,
        id: &str,
        text: &str,
    ) -> anyhow::Result<VoiceDelegationAdmission> {
        self.admit_input(id, text, AnalystTurnOrigin::Terminal)
            .await
    }
}
