use super::*;

const MAX_QUEUED_INPUTS: usize = 32;

#[derive(Debug)]
pub(super) struct QueuedInput {
    pub delegation_id: String,
    pub text: String,
    pub origin: AnalystTurnOrigin,
}

pub(super) fn start(lifecycle: &Arc<AnalystLifecycle>) {
    let weak = Arc::downgrade(lifecycle);
    let notify = Arc::clone(&lifecycle.queue_notify);
    let task = tokio::spawn(async move {
        loop {
            notify.notified().await;
            let Some(lifecycle) = weak.upgrade() else {
                break;
            };
            loop {
                let mut state = lifecycle.state.lock().await;
                if state.invalidated || state.active.is_some() || state.pending.is_some() {
                    break;
                }
                let Some(input) = state.queue.pop_front() else {
                    break;
                };
                lifecycle
                    .queue_len
                    .store(state.queue.len(), std::sync::atomic::Ordering::Release);
                match lifecycle
                    .start_new(&input.delegation_id, &input.text, input.origin, state)
                    .await
                {
                    Ok(_) => {}
                    Err(error) if super::turns::is_would_block(&error) => {
                        let mut state = lifecycle.state.lock().await;
                        state.queue.push_front(input);
                        lifecycle
                            .queue_len
                            .store(state.queue.len(), std::sync::atomic::Ordering::Release);
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(%error,"Queued ACP input could not be admitted; no replay");
                        let cancelled = error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::Interrupted);
                        let diagnostic = error.root_cause().to_string();
                        lifecycle.complete_unadmitted_input(
                            &input.delegation_id,
                            input.origin,
                            if cancelled { "cancelled" } else { "failed" },
                            if cancelled { "" } else { &diagnostic },
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

impl AnalystLifecycle {
    pub(super) fn complete_unadmitted_input(
        &self,
        id: &str,
        origin: AnalystTurnOrigin,
        status: &str,
        error: &str,
    ) {
        // Host outcomes have no provider receipt. One identity per broadcast
        // gives both Web consumers the same distinct persistence key.
        let _ = self.events.send(AnalystLifecycleEvent::Completed {
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
        mut state: tokio::sync::MutexGuard<'_, LifecycleState>,
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
        state.queue.push_back(QueuedInput {
            delegation_id: id.to_owned(),
            text: text.to_owned(),
            origin,
        });
        state
            .acp_inputs
            .insert(id.to_owned(), (fingerprint, origin));
        let position = state.queue.len();
        self.queue_len
            .store(position, std::sync::atomic::Ordering::Release);
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
