//! Bounded v1 Direct framing; shared invitations keep the original frame format.
use cccc_contracts::connect::ConnectPeerRequest;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) const MAX_FRAME: usize = 14 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Frame {
    Request { request: Box<ConnectPeerRequest> },
    Response { id: String, response: Value },
    Ping,
}
pub(super) async fn read<T: DeserializeOwned>(
    stream: &mut (impl AsyncRead + Unpin),
    limit: usize,
) -> Result<T, String> {
    let length = stream
        .read_u32()
        .await
        .map_err(|_| "Direct channel closed")? as usize;
    if length == 0 || length > limit {
        return Err("Direct frame exceeds its limit".into());
    }
    let mut raw = vec![0; length];
    stream
        .read_exact(&mut raw)
        .await
        .map_err(|_| "Direct frame was interrupted")?;
    serde_json::from_slice(&raw).map_err(|_| "Invalid direct frame".into())
}
pub(super) async fn write(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
) -> Result<(), String> {
    let raw = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if raw.len() > MAX_FRAME {
        return Err("Direct frame exceeds its limit".into());
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        stream.write_u32(raw.len() as u32).await?;
        stream.write_all(&raw).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| "Direct write timed out")?
    .map_err(|_| "Direct write was interrupted".into())
}
