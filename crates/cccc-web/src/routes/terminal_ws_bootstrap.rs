use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};

pub(super) const SNAPSHOT_V1: &str = "snapshot_v1";
const MAX_SNAPSHOT_FRAME_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SnapshotBootstrap {
    pub(super) bytes: usize,
    pub(super) cursor: u64,
}

pub(super) fn snapshot_bootstrap(
    result: &Value,
) -> Result<Option<SnapshotBootstrap>, &'static str> {
    let Some(initial) = result.get("initial_output") else {
        return Ok(None);
    };
    if initial.get("kind").and_then(Value::as_str) != Some("snapshot") {
        return Ok(None);
    }
    let bytes = initial
        .get("bytes")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| (1..=MAX_SNAPSHOT_FRAME_BYTES).contains(value))
        .ok_or("Terminal snapshot has an invalid byte length.")?;
    let cursor = initial
        .get("cursor")
        .and_then(Value::as_u64)
        .ok_or("Terminal snapshot is missing its cursor.")?;
    if result.get("replay_cursor").and_then(Value::as_u64) != Some(cursor)
        || result.get("replay_end_cursor").and_then(Value::as_u64) != Some(cursor)
    {
        return Err("Terminal snapshot cursor does not match the attach cursor.");
    }
    Ok(Some(SnapshotBootstrap { bytes, cursor }))
}

pub(super) async fn read_snapshot<R>(
    read: &mut R,
    snapshot: SnapshotBootstrap,
) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut data = vec![0; snapshot.bytes];
    read.read_exact(&mut data).await?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_only_a_bounded_snapshot_with_an_exact_cursor_fence() {
        let value = json!({
            "replay_cursor": 42,
            "replay_end_cursor": 42,
            "initial_output": {"kind":"snapshot","bytes":128,"cursor":42},
        });
        assert_eq!(
            snapshot_bootstrap(&value),
            Ok(Some(SnapshotBootstrap {
                bytes: 128,
                cursor: 42
            }))
        );

        let mismatched = json!({
            "replay_cursor": 41,
            "replay_end_cursor": 42,
            "initial_output": {"kind":"snapshot","bytes":128,"cursor":42},
        });
        assert!(snapshot_bootstrap(&mismatched).is_err());
    }

    #[test]
    fn leaves_raw_replay_and_older_daemons_untouched() {
        assert_eq!(snapshot_bootstrap(&json!({})), Ok(None));
        assert_eq!(
            snapshot_bootstrap(&json!({"initial_output":{"kind":"replay","bytes":99}})),
            Ok(None)
        );
    }
}
