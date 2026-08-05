use anyhow::{Context, Result};
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

use crate::dispatch::dispatch;
use crate::dispatch_concurrency::DispatchLocks;

const MAX_REQUEST_BYTES: usize = 2_000_000;
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub fn spawn_connection<S>(
    stream: S,
    home: HomeLayout,
    shutdown: watch::Sender<bool>,
    dispatch_locks: DispatchLocks,
) -> tokio::task::JoinHandle<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = handle(stream, home, &shutdown, &dispatch_locks).await {
            tracing::warn!(%error, "daemon connection failed");
        }
    })
}

async fn handle<S>(
    stream: S,
    home: HomeLayout,
    shutdown: &watch::Sender<bool>,
    dispatch_locks: &DispatchLocks,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut read = BufReader::new(read);
    loop {
        let mut bytes = Vec::new();
        let mut limited = (&mut read).take((MAX_REQUEST_BYTES + 1) as u64);
        let count =
            tokio::time::timeout(REQUEST_READ_TIMEOUT, limited.read_until(b'\n', &mut bytes))
                .await
                .context("daemon request read timed out")??;
        if count == 0 {
            break;
        }
        let oversized = bytes.len() > MAX_REQUEST_BYTES;
        let response = if oversized {
            DaemonResponse::failure("request_too_large", "request exceeds 2 MB")
        } else {
            response(&home, &bytes, shutdown, dispatch_locks).await
        };
        let mut payload = serde_json::to_vec(&response)?;
        payload.push(b'\n');
        write.write_all(&payload).await?;
        write.flush().await?;
        if oversized {
            break;
        }
    }
    Ok(())
}

async fn response(
    home: &HomeLayout,
    bytes: &[u8],
    shutdown: &watch::Sender<bool>,
    dispatch_locks: &DispatchLocks,
) -> DaemonResponse {
    let request = match serde_json::from_slice::<DaemonRequest>(bytes) {
        Ok(request) => request,
        Err(error) => return DaemonResponse::failure("invalid_request", error.to_string()),
    };
    let should_shutdown = request.op == "shutdown";
    let permit = dispatch_locks.acquire(&request).await;
    let home = home.clone();
    let request_for_dispatch = request.clone();
    let response = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        dispatch(&home, &request_for_dispatch)
    })
    .await
    .unwrap_or_else(|error| DaemonResponse::failure("dispatch_failed", error.to_string()));
    if should_shutdown && response.ok {
        shutdown.send(true).ok();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::handle;
    use crate::dispatch_concurrency::DispatchLocks;
    use cccc_core::HomeLayout;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::watch;

    #[tokio::test]
    async fn malformed_connection_does_not_panic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let (mut client, server) = tokio::io::duplex(1024);
        let (shutdown, _) = watch::channel(false);
        let lock = DispatchLocks::default();
        let task = tokio::spawn(async move { handle(server, home, &shutdown, &lock).await });
        client.write_all(b"not-json\n").await.expect("write");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .expect("read");
        assert!(response.contains("invalid_request"));
        client.shutdown().await.expect("shutdown");
        assert!(task.await.expect("join").is_ok());
    }

    #[tokio::test]
    async fn connection_handles_multiple_requests() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let (mut client, server) = tokio::io::duplex(4096);
        let (shutdown, _) = watch::channel(false);
        let lock = DispatchLocks::default();
        let task = tokio::spawn(async move { handle(server, home, &shutdown, &lock).await });
        client
            .write_all(
                b"{\"v\":1,\"op\":\"ping\",\"args\":{}}\n{\"v\":1,\"op\":\"ping\",\"args\":{}}\n",
            )
            .await
            .expect("write");
        let mut reader = BufReader::new(&mut client);
        for _ in 0..2 {
            let mut response = String::new();
            reader.read_line(&mut response).await.expect("read");
            let response: cccc_contracts::DaemonResponse =
                serde_json::from_str(&response).expect("response");
            assert!(response.ok);
        }
        drop(reader);
        client.shutdown().await.expect("shutdown");
        assert!(task.await.expect("join").is_ok());
    }
}
