use serde_json::Value;
use serde_json::json;

use super::terminal_ws_protocol::daemon_call;
use crate::AppState;

const TERMINAL_INITIAL_REPLAY_LIMIT_BYTES: usize = 512 * 1024;
const TERMINAL_POLL_LIMIT_BYTES: usize = 64_000;

pub(super) struct PolledOutput {
    pub(super) data: Vec<u8>,
    pub(super) replay_cursor: u64,
    pub(super) next_cursor: u64,
    pub(super) replay_end_cursor: u64,
}

pub(super) async fn initial_output(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    requested: Option<u64>,
) -> Option<PolledOutput> {
    replay_output(state, group_id, actor_id, requested, None).await
}

pub(super) async fn replay_output(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    after: Option<u64>,
    end_cursor: Option<u64>,
) -> Option<PolledOutput> {
    let response = daemon_call(
        state,
        "terminal_replay",
        replay_args(group_id, actor_id, after, end_cursor),
    )
    .await?;
    if !response.ok {
        return None;
    }
    let replay_end_cursor = response.result.get("replay_end_cursor")?.as_u64()?;
    history_window(response.result.get("history")?, Some(replay_end_cursor))
}

pub(super) async fn poll_output(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    cursor: u64,
) -> Option<PolledOutput> {
    let response = daemon_call(
        state,
        "terminal_since",
        json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "after":cursor,
            "limit_bytes":TERMINAL_POLL_LIMIT_BYTES,
        }),
    )
    .await?;
    if !response.ok {
        return None;
    }
    history_window(response.result.get("history")?, None)
}

fn history_window(history: &Value, replay_end_cursor: Option<u64>) -> Option<PolledOutput> {
    let next_cursor = history.get("end_cursor")?.as_u64()?;
    Some(PolledOutput {
        data: history.get("data")?.as_str()?.as_bytes().to_vec(),
        replay_cursor: history.get("start_cursor")?.as_u64()?,
        next_cursor,
        replay_end_cursor: replay_end_cursor.unwrap_or(next_cursor),
    })
}

fn replay_args(
    group_id: &str,
    actor_id: &str,
    after: Option<u64>,
    end_cursor: Option<u64>,
) -> Value {
    let mut args = json!({
        "group_id":group_id,
        "actor_id":actor_id,
        "limit_bytes":TERMINAL_INITIAL_REPLAY_LIMIT_BYTES,
    });
    if let Some(after) = after {
        args["after"] = json!(after);
    }
    if let Some(end_cursor) = end_cursor {
        args["end_cursor"] = json!(end_cursor);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::{TERMINAL_INITIAL_REPLAY_LIMIT_BYTES, history_window, replay_args};
    use serde_json::json;

    #[test]
    fn initial_history_preserves_raw_ansi_scrollback_and_absolute_cursors() {
        let raw = "old conversation\r\n\u{1b}[2J\u{1b}[Hcurrent screen";
        let output = history_window(
            &json!({
                "data": raw,
                "start_cursor": 41,
                "end_cursor": 41 + raw.len(),
                "has_more": true,
            }),
            Some(600_000),
        )
        .expect("raw history");

        assert_eq!(output.data, raw.as_bytes());
        assert_eq!(output.replay_cursor, 41);
        assert_eq!(output.next_cursor, 41 + raw.len() as u64);
        assert_eq!(output.replay_end_cursor, 600_000);
        assert_eq!(TERMINAL_INITIAL_REPLAY_LIMIT_BYTES, 512 * 1024);
    }

    #[test]
    fn replay_requests_are_current_session_only_and_cursor_aware() {
        let first = replay_args("g1", "peer1", None, None);
        let next = replay_args("g1", "peer1", Some(524_288), Some(600_000));

        assert_eq!(first["group_id"], "g1");
        assert_eq!(first["actor_id"], "peer1");
        assert_eq!(first["limit_bytes"], 512 * 1024);
        assert!(first.get("after").is_none());
        assert!(first.get("end_cursor").is_none());
        assert_eq!(next["after"], 524_288);
        assert_eq!(next["end_cursor"], 600_000);
    }
}
