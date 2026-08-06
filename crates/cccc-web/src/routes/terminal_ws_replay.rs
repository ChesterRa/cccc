use serde_json::json;

use super::terminal_ws_protocol::daemon_call;
use crate::AppState;

const TERMINAL_POLL_LIMIT_BYTES: usize = 64_000;

pub(super) struct PolledOutput {
    pub(super) data: Vec<u8>,
    pub(super) replay_cursor: u64,
    pub(super) next_cursor: u64,
}

pub(super) async fn initial_output(
    state: &AppState,
    group_id: &str,
    actor_id: &str,
    requested: Option<u64>,
) -> Option<PolledOutput> {
    if let Some(cursor) = requested {
        return poll_output(state, group_id, actor_id, cursor).await;
    }
    let response = daemon_call(
        state,
        "terminal_snapshot",
        json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "limit_bytes":512 * 1024,
        }),
    )
    .await?;
    if !response.ok {
        return None;
    }
    match snapshot_window(
        response.result.get("data")?.as_str()?.as_bytes().to_vec(),
        response.result.get("end_cursor")?.as_u64()?,
    ) {
        Some(snapshot) => Some(snapshot),
        None => poll_output(state, group_id, actor_id, 0).await,
    }
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
    let history = response.result.get("history")?;
    Some(PolledOutput {
        data: history.get("data")?.as_str()?.as_bytes().to_vec(),
        replay_cursor: history.get("start_cursor")?.as_u64()?,
        next_cursor: history.get("end_cursor")?.as_u64()?,
    })
}

fn snapshot_window(data: Vec<u8>, end: u64) -> Option<PolledOutput> {
    let payload_bytes = u64::try_from(data.len()).ok()?;
    if payload_bytes > end {
        return None;
    }
    Some(PolledOutput {
        data,
        replay_cursor: end - payload_bytes,
        next_cursor: end,
    })
}

#[cfg(test)]
mod tests {
    use super::snapshot_window;

    #[test]
    fn snapshot_uses_a_compatible_synthetic_replay_cursor() {
        let output = snapshot_window(b"screen".to_vec(), 20).expect("snapshot");
        assert_eq!(output.replay_cursor, 14);
        assert_eq!(output.next_cursor, 20);
        assert_eq!(output.data, b"screen");
        assert!(snapshot_window(b"too long".to_vec(), 2).is_none());
    }
}
