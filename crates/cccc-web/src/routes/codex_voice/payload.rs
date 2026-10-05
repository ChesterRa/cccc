use crate::codex_voice::{AnalystInfo, SessionInfo};
use serde_json::{Value, json};

pub(super) fn info_value(info: SessionInfo) -> Value {
    json!({
        "generation":info.generation,
        "analyst_generation":info.analyst_generation,
        "mode":info.mode,
        "voice":info.voice,
        "connected":info.connected,
    })
}

pub(super) fn analyst_info_value(info: AnalystInfo) -> Value {
    json!({
        "generation":info.generation,
        "tui_ready":info.tui_ready,
        "phase":info.phase,
        "last_result":info.last_result,
        "warning":info.warning,
        "structured":info.structured,
        "queued_inputs":info.queued_inputs,
        "permissions":info.permissions,
        "progress":info.progress,
        "last_error":info.last_error,
        "manual_tasks":info.manual_tasks,
        "manual_task_id":info.manual_task_id,
    })
}
