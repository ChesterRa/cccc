use crate::codex_voice::{AnalystInfo, SessionInfo};
use serde_json::{Value, json};

pub(super) fn info_value(info: SessionInfo) -> Value {
    json!({
        "group_id":info.group_id,
        "group_title":info.group_title,
        "generation":info.generation,
        "analyst_generation":info.analyst_generation,
        "voice":info.voice,
        "connected":info.connected,
    })
}

pub(super) fn analyst_info_value(info: AnalystInfo) -> Value {
    json!({
        "group_id":info.group_id,
        "group_title":info.group_title,
        "generation":info.generation,
        "tui_ready":info.tui_ready,
        "phase":info.phase,
        "last_result":info.last_result,
        "warning":info.warning,
    })
}
