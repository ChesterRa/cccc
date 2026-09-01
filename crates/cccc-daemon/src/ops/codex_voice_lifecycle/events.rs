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
            tracing::warn!("ignored Voice Analyst event for a different Codex thread");
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
        if method == "cccc/voiceAnalyst/disconnected" {
            self.invalidate().await;
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
        // A real turn on a legacy-history thread is the Codex materialization boundary. Mark it
        // before publishing lifecycle state so the Web TUI endpoint cannot lag behind its button.
        self.session.mark_thread_materialized();
        let mut state = self.state.lock().await;
        if state.active.is_some() {
            return;
        }
        let (delegation_id, origin) = state
            .pending
            .as_ref()
            .filter(|pending| {
                event.requested_delegation_id.as_deref() == Some(pending.delegation_id.as_str())
            })
            .map(|pending| (pending.delegation_id.clone(), pending.origin))
            .unwrap_or_else(|| (String::new(), AnalystTurnOrigin::Terminal));
        state.active = Some(ActiveTurn {
            turn_id: turn_id.to_owned(),
            latest_delegation_id: delegation_id,
            origin,
            cancelling: false,
            deltas: String::new(),
            completed_text: String::new(),
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
        let _ = self
            .events
            .send(AnalystLifecycleEvent::Started { receipt, origin });
    }

    async fn handle_turn_event(&self, method: &str, params: &Value) {
        let mut state = self.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            return;
        };
        if method == "item/agentMessage/delta" && params["turnId"] == active.turn_id {
            if let Some(delta) = params["delta"].as_str()
                && active.deltas.len().saturating_add(delta.len()) <= MAX_RESULT_BYTES
            {
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
            active.completed_text = truncate_utf8(
                params["item"]["text"].as_str().unwrap_or_default(),
                MAX_RESULT_BYTES,
            );
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
        let result = if active.completed_text.trim().is_empty() {
            active.deltas.trim().to_owned()
        } else {
            active.completed_text.trim().to_owned()
        };
        let _ = self.events.send(AnalystLifecycleEvent::Completed {
            turn_id: active.turn_id,
            delegation_id: active.latest_delegation_id,
            status: params["turn"]["status"]
                .as_str()
                .unwrap_or("failed")
                .to_owned(),
            result,
            speakable: active.origin.speakable(),
        });
    }
}

pub(super) fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
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
