use super::started_turn_id;
use serde_json::Value;
use std::{collections::VecDeque, io};
use tokio::sync::oneshot;

pub(super) struct PendingResponse {
    pub(super) response: oneshot::Sender<io::Result<Value>>,
    pub(super) turn_delegation_id: Option<String>,
    /// Thread the correlated `turn/start` targeted; sub-agent threads announce
    /// their own turns and must not count as competing with it.
    pub(super) thread_id: Option<String>,
}

impl PendingResponse {
    pub(super) fn new(
        response: oneshot::Sender<io::Result<Value>>,
        turn_delegation_id: Option<String>,
        params: &Value,
    ) -> Self {
        let thread_id = params
            .get("threadId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Self {
            response,
            turn_delegation_id,
            thread_id,
        }
    }
}

/// Whether a deferred `turn/started` names a different turn on the same
/// thread as the pending `turn/start`. Turns on other threads (Codex
/// sub-agents) are not competition for the correlated request.
pub(super) fn competing_turn_started(
    deferred_events: &VecDeque<Value>,
    response_turn_id: &str,
    thread_id: Option<&str>,
) -> bool {
    deferred_events.iter().any(|event| {
        let same_thread = match (thread_id, event_thread_id(event)) {
            (Some(expected), Some(actual)) => expected == actual,
            _ => true,
        };
        same_thread && started_turn_id(event).is_some_and(|turn_id| turn_id != response_turn_id)
    })
}

fn event_thread_id(message: &Value) -> Option<&str> {
    message
        .pointer("/params/threadId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod competing_turn_tests {
    use super::*;
    use serde_json::json;

    fn started(thread_id: &str, turn_id: &str) -> Value {
        json!({"method":"turn/started","params":{"threadId":thread_id,"turn":{"id":turn_id}}})
    }

    #[test]
    fn a_sub_agent_turn_does_not_compete_with_the_pending_turn() {
        let deferred = VecDeque::from([started("thread-sub", "turn-sub")]);
        assert!(!competing_turn_started(
            &deferred,
            "turn-main",
            Some("thread-main")
        ));
    }

    #[test]
    fn another_turn_on_the_same_thread_still_competes() {
        let deferred = VecDeque::from([started("thread-main", "turn-other")]);
        assert!(competing_turn_started(
            &deferred,
            "turn-main",
            Some("thread-main")
        ));
        assert!(!competing_turn_started(
            &deferred,
            "turn-other",
            Some("thread-main")
        ));
    }

    #[test]
    fn unknown_threads_keep_the_conservative_check() {
        let untagged = VecDeque::from([
            json!({"method":"turn/started","params":{"turn":{"id":"turn-other"}}}),
        ]);
        assert!(competing_turn_started(
            &untagged,
            "turn-main",
            Some("thread-main")
        ));
        let tagged = VecDeque::from([started("thread-sub", "turn-other")]);
        assert!(competing_turn_started(&tagged, "turn-main", None));
    }
}
