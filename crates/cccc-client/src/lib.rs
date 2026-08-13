use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;

mod connection;
use connection::Connection;
pub use connection::DaemonStream;

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
    #[error("daemon request outcome is unknown for {op}: {message}")]
    OutcomeUnknown { op: String, message: String },
}

#[derive(Debug, Clone)]
pub struct DaemonClient {
    home: HomeLayout,
    timeout: Duration,
    shared: Arc<ClientShared>,
}

#[derive(Debug, Default)]
struct ClientShared {
    address: RwLock<Option<DaemonAddress>>,
}

#[derive(Debug)]
enum CallFailure {
    Connect(ClientError),
    Exchange(ClientError),
}

impl CallFailure {
    fn into_client_error(self) -> ClientError {
        match self {
            Self::Connect(error) | Self::Exchange(error) => error,
        }
    }
}

impl DaemonClient {
    #[must_use]
    pub fn new(home: HomeLayout) -> Self {
        Self {
            home,
            timeout: Duration::from_secs(60),
            shared: Arc::new(ClientShared::default()),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn call(&self, request: &DaemonRequest) -> Result<DaemonResponse, ClientError> {
        let exchange_started = AtomicBool::new(false);
        match tokio::time::timeout(self.timeout, self.call_inner(request, &exchange_started)).await
        {
            Ok(result) => result,
            Err(_) if exchange_started.load(Ordering::Acquire) => {
                Err(ClientError::OutcomeUnknown {
                    op: request.op.clone(),
                    message: "request timed out after exchange started".into(),
                })
            }
            Err(_) => Err(ClientError::Timeout),
        }
    }

    /// Opens a dedicated connection and preserves it after the response line
    /// for protocol operations that upgrade to a raw stream.
    pub async fn upgrade(
        &self,
        request: &DaemonRequest,
    ) -> Result<(DaemonResponse, DaemonStream), ClientError> {
        let exchange_started = AtomicBool::new(false);
        match tokio::time::timeout(self.timeout, self.upgrade_inner(request, &exchange_started))
            .await
        {
            Ok(result) => result,
            Err(_) if exchange_started.load(Ordering::Acquire) => {
                Err(ClientError::OutcomeUnknown {
                    op: request.op.clone(),
                    message: "request timed out after exchange started".into(),
                })
            }
            Err(_) => Err(ClientError::Timeout),
        }
    }

    async fn call_inner(
        &self,
        request: &DaemonRequest,
        exchange_started: &AtomicBool,
    ) -> Result<DaemonResponse, ClientError> {
        match self.call_once(request, exchange_started).await {
            Err(CallFailure::Connect(ClientError::Transport(_))) => {
                self.invalidate_transport().await;
                match self.call_once(request, exchange_started).await {
                    Err(CallFailure::Exchange(error)) => {
                        self.invalidate_transport().await;
                        Err(outcome_unknown(request, error))
                    }
                    Err(error) => Err(error.into_client_error()),
                    Ok(response) => Ok(response),
                }
            }
            Err(CallFailure::Exchange(error)) => {
                self.invalidate_transport().await;
                Err(outcome_unknown(request, error))
            }
            Err(error) => Err(error.into_client_error()),
            Ok(response) => Ok(response),
        }
    }

    async fn upgrade_inner(
        &self,
        request: &DaemonRequest,
        exchange_started: &AtomicBool,
    ) -> Result<(DaemonResponse, DaemonStream), ClientError> {
        match self.upgrade_once(request, exchange_started).await {
            Err(CallFailure::Connect(ClientError::Transport(_))) => {
                self.invalidate_transport().await;
                match self.upgrade_once(request, exchange_started).await {
                    Err(CallFailure::Exchange(error)) => {
                        self.invalidate_transport().await;
                        Err(outcome_unknown(request, error))
                    }
                    Err(error) => Err(error.into_client_error()),
                    Ok(result) => Ok(result),
                }
            }
            Err(CallFailure::Exchange(error)) => {
                self.invalidate_transport().await;
                Err(outcome_unknown(request, error))
            }
            Err(error) => Err(error.into_client_error()),
            Ok(result) => Ok(result),
        }
    }

    async fn upgrade_once(
        &self,
        request: &DaemonRequest,
        exchange_started: &AtomicBool,
    ) -> Result<(DaemonResponse, DaemonStream), CallFailure> {
        let mut connection = self.connect().await.map_err(CallFailure::Connect)?;
        exchange_started.store(true, Ordering::Release);
        let response = connection
            .exchange(request)
            .await
            .map_err(CallFailure::Exchange)?;
        Ok((response, DaemonStream::new(connection)))
    }

    async fn call_once(
        &self,
        request: &DaemonRequest,
        exchange_started: &AtomicBool,
    ) -> Result<DaemonResponse, CallFailure> {
        let mut connection = self.connect().await.map_err(CallFailure::Connect)?;
        exchange_started.store(true, Ordering::Release);
        connection
            .exchange(request)
            .await
            .map_err(CallFailure::Exchange)
    }

    async fn connect(&self) -> Result<Connection, ClientError> {
        let address = self.address().await?;
        Connection::connect(&address).await
    }

    async fn address(&self) -> Result<DaemonAddress, ClientError> {
        if let Some(address) = self.shared.address.read().await.clone() {
            return Ok(address);
        }
        let path = self.home.daemon_dir().join("ccccd.addr.json");
        let raw = tokio::fs::read(&path)
            .await
            .map_err(|_| ClientError::AddressUnavailable(path))?;
        let address: DaemonAddress = serde_json::from_slice(&raw)?;
        *self.shared.address.write().await = Some(address.clone());
        Ok(address)
    }

    async fn invalidate_transport(&self) {
        *self.shared.address.write().await = None;
    }
}

fn outcome_unknown(request: &DaemonRequest, error: ClientError) -> ClientError {
    match error {
        ClientError::Transport(error) => ClientError::OutcomeUnknown {
            op: request.op.clone(),
            message: error.to_string(),
        },
        ClientError::Protocol(error) => ClientError::OutcomeUnknown {
            op: request.op.clone(),
            message: error.to_string(),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests;
