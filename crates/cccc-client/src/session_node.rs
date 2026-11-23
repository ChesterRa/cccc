//! Authenticated, loopback-only bridge into the foreground native node connector.
use cccc_core::session_nodes::ClientDescriptor;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{io, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_FRAME: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    pub secret: String,
    pub source_id: String,
    pub revision: String,
    pub request: Value,
}

pub async fn call(
    client: &ClientDescriptor,
    source_id: &str,
    revision: &str,
    request: Value,
) -> io::Result<Value> {
    if !client.address.ip().is_loopback() || !client.connected || !client.owner.alive() {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "Remote-session connector is offline",
        ));
    }
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut stream = tokio::net::TcpStream::connect(client.address).await?;
        write(
            &mut stream,
            &ToolRequest {
                secret: client.secret.clone(),
                source_id: source_id.into(),
                revision: revision.into(),
                request,
            },
        )
        .await?;
        read(&mut stream).await
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "Remote tool outcome is unknown; do not automatically repeat a write",
        )
    })?
}

pub async fn write<T: Serialize>(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Session-node frame exceeds one MiB",
        ));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await
}

pub async fn read<T: DeserializeOwned>(stream: &mut (impl AsyncRead + Unpin)) -> io::Result<T> {
    let len = stream.read_u32().await? as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Session-node frame exceeds one MiB",
        ));
    }
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
