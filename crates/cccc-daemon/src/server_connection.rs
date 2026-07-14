use anyhow::{Context, Result};
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, watch};

use crate::dispatch::dispatch;

const MAX_REQUEST_BYTES: usize = 2_000_000;
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub type DispatchLock = Arc<Mutex<()>>;

pub fn spawn_connection<S>(
    stream: S,
    home: HomeLayout,
    shutdown: watch::Sender<bool>,
    dispatch_lock: DispatchLock,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = handle(stream, home, &shutdown, &dispatch_lock).await {
            tracing::warn!(%error, "daemon connection failed");
        }
    });
}

async fn handle<S>(
    stream: S,
    home: HomeLayout,
    shutdown: &watch::Sender<bool>,
    dispatch_lock: &Mutex<()>,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut bytes = Vec::new();
    let mut limited = BufReader::new(read).take((MAX_REQUEST_BYTES + 1) as u64);
    let read = limited.read_until(b'\n', &mut bytes);
    tokio::time::timeout(REQUEST_READ_TIMEOUT, read)
        .await
        .context("daemon request read timed out")??;
    let response = if bytes.len() > MAX_REQUEST_BYTES {
        DaemonResponse::failure("request_too_large", "request exceeds 2 MB")
    } else {
        response(&home, &bytes, shutdown, dispatch_lock).await
    };
    let mut payload = serde_json::to_vec(&response)?;
    payload.push(b'\n');
    write.write_all(&payload).await?;
    write.shutdown().await?;
    Ok(())
}

async fn response(
    home: &HomeLayout,
    bytes: &[u8],
    shutdown: &watch::Sender<bool>,
    dispatch_lock: &Mutex<()>,
) -> DaemonResponse {
    let request = match serde_json::from_slice::<DaemonRequest>(bytes) {
        Ok(request) => request,
        Err(error) => return DaemonResponse::failure("invalid_request", error.to_string()),
    };
    let should_shutdown = request.op == "shutdown";
    let _guard = dispatch_lock.lock().await;
    let response = dispatch(home, &request);
    if should_shutdown && response.ok {
        shutdown.send(true).ok();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{DispatchLock, handle};
    use cccc_core::HomeLayout;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{Mutex, watch};

    #[tokio::test]
    async fn malformed_connection_does_not_panic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let (mut client, server) = tokio::io::duplex(1024);
        let (shutdown, _) = watch::channel(false);
        let lock: DispatchLock = Arc::new(Mutex::new(()));
        let task = tokio::spawn(async move { handle(server, home, &shutdown, &lock).await });
        client.write_all(b"not-json\n").await.expect("write");
        let mut response = String::new();
        client.read_to_string(&mut response).await.expect("read");
        assert!(response.contains("invalid_request"));
        assert!(task.await.expect("join").is_ok());
    }
}
