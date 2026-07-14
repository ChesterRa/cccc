use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport};
use cccc_core::HomeLayout;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[cfg(unix)]
use tokio::net::UnixStream;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("daemon address unavailable at {0}")]
    AddressUnavailable(PathBuf),
    #[error("invalid daemon address: {0}")]
    InvalidAddress(String),
    #[error("daemon transport failed: {0}")]
    Transport(#[from] std::io::Error),
    #[error("daemon protocol failed: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("daemon request timed out")]
    Timeout,
}

#[derive(Debug, Clone)]
pub struct DaemonClient {
    home: HomeLayout,
    timeout: Duration,
}

impl DaemonClient {
    #[must_use]
    pub fn new(home: HomeLayout) -> Self {
        Self {
            home,
            timeout: Duration::from_secs(60),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn call(&self, request: &DaemonRequest) -> Result<DaemonResponse, ClientError> {
        tokio::time::timeout(self.timeout, self.call_inner(request))
            .await
            .map_err(|_| ClientError::Timeout)?
    }

    async fn call_inner(&self, request: &DaemonRequest) -> Result<DaemonResponse, ClientError> {
        let address = self.read_address().await?;
        match address.transport {
            Transport::Tcp => {
                if address.host.is_empty() || address.port == 0 {
                    return Err(ClientError::InvalidAddress(
                        "missing TCP host or port".into(),
                    ));
                }
                let stream = TcpStream::connect((address.host.as_str(), address.port)).await?;
                exchange(stream, request).await
            }
            Transport::Unix => self.call_unix(&address, request).await,
        }
    }

    #[cfg(unix)]
    async fn call_unix(
        &self,
        address: &DaemonAddress,
        request: &DaemonRequest,
    ) -> Result<DaemonResponse, ClientError> {
        if address.path.is_empty() {
            return Err(ClientError::InvalidAddress(
                "missing Unix socket path".into(),
            ));
        }
        exchange(UnixStream::connect(&address.path).await?, request).await
    }

    #[cfg(not(unix))]
    async fn call_unix(
        &self,
        _address: &DaemonAddress,
        _request: &DaemonRequest,
    ) -> Result<DaemonResponse, ClientError> {
        Err(ClientError::InvalidAddress(
            "Unix sockets are unsupported".into(),
        ))
    }

    async fn read_address(&self) -> Result<DaemonAddress, ClientError> {
        let path = self.home.daemon_dir().join("ccccd.addr.json");
        let raw = tokio::fs::read(&path)
            .await
            .map_err(|_| ClientError::AddressUnavailable(path))?;
        Ok(serde_json::from_slice(&raw)?)
    }
}

async fn exchange<S>(stream: S, request: &DaemonRequest) -> Result<DaemonResponse, ClientError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut payload = serde_json::to_vec(request)?;
    payload.push(b'\n');
    write.write_all(&payload).await?;
    write.shutdown().await?;

    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    if line.is_empty() {
        return Err(ClientError::InvalidAddress(
            "daemon closed without response".into(),
        ));
    }
    Ok(serde_json::from_str(&line)?)
}
