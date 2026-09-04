use super::*;
use serde_json::Value;

const MAX_RESULT_BYTES: usize = 32 * 1024;

impl AnalystLifecycle {
    pub(super) async fn handle(&self, event: AnalystEvent) {
        if event.generation != self.session.generation() {
            return;
        }
        if event
            .message
            .get("params")
            .and_then(|params| params.get("threadId"))
            .and_then(Value::as_str)
            .is_some_and(|thread_id| thread_id != self.session.thread_id())
        {
            tracing::warn!("ignored Voice Analyst event for a different managed session");
            return;
        }
        let method = event.message["method"].as_str().unwrap_or_default();
        let params = &event.message["params"];
        if let Some(work) = tracked_work(&event.message) {
            let event_turn_id = params["turnId"].as_str().unwrap_or_default();
            let belongs_to_voice_turn =
                self.state
                    .lock()
                    .await
                    .active
                    .as_ref()
                    .is_some_and(|active| {
                        active.origin == AnalystTurnOrigin::Voice && active.turn_id == event_turn_id
                    });
            if belongs_to_voice_turn {
                let _ = self.events.send(AnalystLifecycleEvent::TrackedWork(work));
            }
        }
        if method == "mcpServer/elicitation/request" {
            let _ = self
                .session
                .respond_mcp_elicitation(
                    self.session.generation(),
                    &event,
                    ElicitationAction::Decline,
                )
                .await;
            let _ = self.cancel_current().await;
            let _ = self.events.send(AnalystLifecycleEvent::NeedsAttention {
                code: "unexpected_approval",
            });
            return;
        }
        if method == super::super::codex_voice_analyst::MANAGED_AGENT_DISCONNECTED_METHOD {
            tracing::warn!(
                reason = params["reason"].as_str().unwrap_or("unspecified"),
                thread_id = %self.session.thread_id(),
                "Voice Analyst managed session disconnected"
            );
            self.invalidate().await;
            if let Err(error) = self.session.stop(self.session.generation()).await {
                tracing::error!(%error, "failed to stop disconnected Voice Analyst session");
            }
            return;
        }
        if method == super::super::codex_voice_analyst::MANAGED_AGENT_DELEGATION_ATTACHED_METHOD {
            self.handle_delegation_attached(&event).await;
            return;
        }
        if method == "turn/started" {
            self.handle_turn_started(&event).await;
            return;
        }
        self.handle_turn_event(method, params).await;
    }

    async fn handle_turn_started(&self, event: &AnalystEvent) {
        let params = &event.message["params"];
        let turn_id = params["turn"]["id"].as_str().unwrap_or_default().trim();
        if turn_id.is_empty() {
            return;
        }
        let mut state = self.state.lock().await;
        if state.active.is_some() {
            if let Some(delegation_id) = event.requested_delegation_id.as_deref() {
                associate_native_delegation(
                    &self.events,
                    self.session.thread_id(),
                    &mut state,
                    turn_id,
                    delegation_id,
                );
            }
            return;
        }
        let controlled = state
            .pending
            .as_ref()
            .filter(|pending| {
                event.requested_delegation_id.as_deref() == Some(pending.delegation_id.as_str())
            })
            .map(|pending| (pending.delegation_id.clone(), pending.origin));
        let native = event
            .requested_delegation_id
            .as_deref()
            .and_then(|delegation_id| {
                take_native_pending(&mut state.native_pending, delegation_id)
                    .map(|pending| (pending.delegation_id, pending.origin))
            });
        // Codex is the only adapter whose native TUI shares the app-server event stream without
        // an explicit user-echo correlation hook. During its narrow start-admission race, the next
        // authoritative TUI turn consumes the oldest registered Voice input.
        let codex_native = (controlled.is_none()
            && native.is_none()
            && state.pending.is_none()
            && self.session.supports_steer())
        .then(|| state.native_pending.pop_front())
        .flatten()
        .map(|pending| (pending.delegation_id, pending.origin));
        let (delegation_id, origin) = controlled
            .or(native)
            .or(codex_native)
            .unwrap_or_else(|| (String::new(), AnalystTurnOrigin::Terminal));
        state.active = Some(ActiveTurn {
            turn_id: turn_id.to_owned(),
            latest_delegation_id: delegation_id,
            origin,
            cancelling: false,
            deltas: String::new(),
            completed_text: String::new(),
            result_overflowed: false,
        });
        let receipt = TurnReceipt {
            delegation_id: state
                .active
                .as_ref()
                .map(|active| active.latest_delegation_id.clone())
                .unwrap_or_default(),
            thread_id: self.session.thread_id().to_owned(),
            turn_id: turn_id.to_owned(),
        };
        if !receipt.delegation_id.is_empty() {
            state
                .delegations
                .insert(receipt.delegation_id.clone(), receipt.clone());
        }
        let _ = self
            .events
            .send(AnalystLifecycleEvent::Started { receipt, origin });
    }

    async fn handle_delegation_attached(&self, event: &AnalystEvent) {
        let turn_id = event.message["params"]["turnId"]
            .as_str()
            .unwrap_or_default()
            .trim();
        let delegation_id = event
            .requested_delegation_id
            .as_deref()
            .unwrap_or_default()
            .trim();
        if turn_id.is_empty() || delegation_id.is_empty() {
            return;
        }
        let mut state = self.state.lock().await;
        associate_native_delegation(
            &self.events,
            self.session.thread_id(),
            &mut state,
            turn_id,
            delegation_id,
        );
    }

    async fn handle_turn_event(&self, method: &str, params: &Value) {
        let mut state = self.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            return;
        };
        if method == "item/agentMessage/delta" && params["turnId"] == active.turn_id {
            if let Some(delta) = params["delta"].as_str() {
                if active.result_overflowed
                    || active.deltas.len().saturating_add(delta.len()) > MAX_RESULT_BYTES
                {
                    active.result_overflowed = true;
                    return;
                }
                active.deltas.push_str(delta);
                let _ = self.events.send(AnalystLifecycleEvent::Progress {
                    turn_id: active.turn_id.clone(),
                    text: delta.to_owned(),
                    speakable: active.origin.speakable(),
                });
            }
            return;
        }
        if method == "item/completed"
            && params["turnId"] == active.turn_id
            && params["item"]["type"] == "agentMessage"
        {
            let text = params["item"]["text"].as_str().unwrap_or_default();
            active.completed_text.clear();
            if !text.trim().is_empty() {
                // A bounded authoritative final can supersede an overlong stream.
                // ACP adapters with no final snapshot must retain the overflow.
                active.result_overflowed = text.len() > MAX_RESULT_BYTES;
                if !active.result_overflowed {
                    active.completed_text.push_str(text);
                }
            }
            return;
        }
        if method != "turn/completed" || params["turn"]["id"] != active.turn_id {
            return;
        }
        let active_origin = active.origin;
        let active_turn_id = active.turn_id.clone();
        let settled_pending = state.pending.as_ref().and_then(|pending| {
            (pending.origin == active_origin).then(|| TurnReceipt {
                delegation_id: pending.delegation_id.clone(),
                thread_id: self.session.thread_id().to_owned(),
                turn_id: active_turn_id.clone(),
            })
        });
        let active = state.active.take().expect("active Voice Analyst turn");
        if settled_pending.is_some() {
            state.settled_pending = settled_pending;
        }
        let result = if active.result_overflowed {
            String::new()
        } else if active.completed_text.trim().is_empty() {
            active.deltas.trim().to_owned()
        } else {
            active.completed_text.trim().to_owned()
        };
        let provider_status = params["turn"]["status"].as_str().unwrap_or("failed");
        let status = if provider_status == "completed" && active.result_overflowed {
            "result_too_large".to_owned()
        } else {
            normalized_completion_status(provider_status, &result, active.origin)
        };
        let _ = self.events.send(AnalystLifecycleEvent::Completed {
            turn_id: active.turn_id,
            delegation_id: active.latest_delegation_id,
            status,
            result,
            speakable: active.origin.speakable(),
        });
    }
}

fn take_native_pending(
    pending: &mut std::collections::VecDeque<PendingStart>,
    delegation_id: &str,
) -> Option<PendingStart> {
    let index = pending
        .iter()
        .position(|pending| pending.delegation_id == delegation_id)?;
    pending.remove(index)
}

fn associate_native_delegation(
    events: &broadcast::Sender<AnalystLifecycleEvent>,
    thread_id: &str,
    state: &mut LifecycleState,
    turn_id: &str,
    delegation_id: &str,
) {
    if state.delegations.contains_key(delegation_id) {
        return;
    }
    if state
        .active
        .as_ref()
        .is_none_or(|active| active.turn_id != turn_id)
    {
        return;
    }
    let Some(pending) = take_native_pending(&mut state.native_pending, delegation_id) else {
        return;
    };
    let active = state.active.as_mut().expect("checked active turn");
    active.latest_delegation_id = delegation_id.to_owned();
    active.origin = pending.origin;
    let receipt = TurnReceipt {
        delegation_id: delegation_id.to_owned(),
        thread_id: thread_id.to_owned(),
        turn_id: turn_id.to_owned(),
    };
    state
        .delegations
        .insert(delegation_id.to_owned(), receipt.clone());
    let _ = events.send(AnalystLifecycleEvent::Associated {
        receipt,
        origin: pending.origin,
    });
}

pub(super) fn normalized_completion_status(
    status: &str,
    result: &str,
    origin: AnalystTurnOrigin,
) -> String {
    if status == "completed" && origin.speakable() && result.trim().is_empty() {
        "failed".into()
    } else {
        status.to_owned()
    }
}

pub(super) fn tracked_work(message: &Value) -> Option<TrackedWork> {
    let item = message.get("params")?.get("item")?;
    if message.get("method")?.as_str()? != "item/completed"
        || item.get("type")?.as_str()? != "mcpToolCall"
        || item.get("status")?.as_str()? != "completed"
        || item.get("server")?.as_str()? != "cccc"
    {
        return None;
    }
    let payload = item.get("result")?.get("structuredContent")?;
    let payload = payload.get("tool_result").unwrap_or(payload);
    let task_id = payload.get("task_id")?.as_str()?.trim();
    let source_event_id = payload.get("event_id")?.as_str()?.trim();
    let recipients = payload.get("event")?.get("data")?.get("to")?.as_array()?;
    let [recipient] = recipients.as_slice() else {
        return None;
    };
    let actor_id = recipient.as_str()?.trim();
    let group_id = payload
        .get("event")
        .and_then(|event| event.get("group_id"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("group_id").and_then(Value::as_str))
        .or_else(|| {
            item.get("arguments")?
                .get("tool_arguments")?
                .get("group_id")?
                .as_str()
        })?
        .trim();
    (!task_id.is_empty()
        && !source_event_id.is_empty()
        && !group_id.is_empty()
        && !actor_id.is_empty()
        && actor_id != "user"
        && !actor_id.starts_with('@'))
    .then(|| TrackedWork {
        group_id: group_id.to_owned(),
        task_id: task_id.to_owned(),
        source_event_id: source_event_id.to_owned(),
        actor_id: actor_id.to_owned(),
    })
}
