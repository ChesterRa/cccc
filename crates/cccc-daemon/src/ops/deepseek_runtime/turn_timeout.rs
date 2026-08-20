use super::RuntimeEntry;
use super::delivery_projection::{TurnProjection, persist_message_completed};
use super::turn_failure::settle_sent_request;
use cccc_runtime::deepseek_supervisor::DeepSeekSupervisor;
use serde_json::{Map, json};

fn append_timeout_terminal(projection: &TurnProjection<'_>) -> std::io::Result<()> {
    crate::ops::local_headless::append_event_with_dedupe(
        projection.home,
        &projection.group.group_id,
        &projection.actor.id,
        "headless.turn.failed",
        Map::from_iter([
            ("event_id".into(), json!(projection.event.id)),
            ("turn_id".into(), json!(projection.turn_id)),
            ("session_id".into(), json!(projection.session_id)),
            ("request_id".into(), json!(projection.request_id)),
            ("status".into(), json!("failed")),
            (
                "error".into(),
                json!({
                    "code": "timeout",
                    "message": "DeepSeek ACP turn timed out"
                }),
            ),
        ]),
        Some(&format!(
            "deepseek.turn:headless.turn.failed:{}:{}",
            projection.event.id, projection.attempt_id
        )),
    )
}

pub(super) fn settle_timed_out_request(
    holder: &RuntimeEntry,
    supervisor: &mut DeepSeekSupervisor,
    projection: &TurnProjection<'_>,
) -> bool {
    let Ok(Some(_terminal)) = settle_sent_request(
        holder,
        supervisor,
        projection.session_id,
        projection.request_id,
        false,
    ) else {
        return false;
    };
    if persist_message_completed(projection).is_err() {
        return false;
    }
    let _ = append_timeout_terminal(projection);
    false
}
