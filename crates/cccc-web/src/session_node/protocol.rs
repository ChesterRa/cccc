use anyhow::{Result, ensure};
use cccc_contracts::session_node::{RemoteSessionDelivery, SessionNodeSource};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::time::Duration;

pub(super) const MAX_FRAME: usize = 1024 * 1024;

#[derive(Debug)]
pub(super) struct RemoteRejection;
impl std::fmt::Display for RemoteRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Remote session-node request was rejected")
    }
}
impl std::error::Error for RemoteRejection {}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Hello {
        version: u32,
        name: String,
        invite_id: Option<String>,
        secret: Option<String>,
    },
    Sync {
        sources: Vec<SessionNodeSource>,
    },
    Tool {
        source_id: String,
        revision: String,
        request: Value,
    },
    Deliver {
        delivery: Box<RemoteSessionDelivery>,
    },
}

pub(super) async fn read<T: DeserializeOwned>(recv: &mut RecvStream) -> Result<T> {
    let bytes =
        tokio::time::timeout(Duration::from_secs(10), recv.read_to_end(MAX_FRAME)).await??;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) async fn write<T: Serialize>(send: &mut SendStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= MAX_FRAME,
        "Session-node response exceeds one MiB"
    );
    tokio::time::timeout(Duration::from_secs(10), send.write_all(&bytes)).await??;
    send.finish()?;
    Ok(())
}

/// No retries: a broken stream after a write has an unknown application outcome.
pub(super) async fn call(connection: &Connection, request: &Request) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(85), async {
        let (mut send, mut recv) = connection.open_bi().await?;
        write(&mut send, request).await?;
        let bytes = recv.read_to_end(MAX_FRAME).await?;
        let response: Value = serde_json::from_slice(&bytes)?;
        if response.get("error").is_some() {
            return Err(RemoteRejection.into());
        }
        Ok(response)
    })
    .await?
}
