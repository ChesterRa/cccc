use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::ClientError;

#[cfg(unix)]
use tokio::net::UnixStream;

#[derive(Debug)]
pub(super) enum Connection {
    Tcp(BufReader<TcpStream>),
    #[cfg(unix)]
    Unix(BufReader<UnixStream>),
}

impl Connection {
    pub(super) async fn connect(address: &DaemonAddress) -> Result<Self, ClientError> {
        match address.transport {
            Transport::Tcp => {
                if address.host.is_empty() || address.port == 0 {
                    return Err(ClientError::InvalidAddress(
                        "missing TCP host or port".into(),
                    ));
                }
                let stream = TcpStream::connect((address.host.as_str(), address.port)).await?;
                Ok(Self::Tcp(BufReader::new(stream)))
            }
            Transport::Unix => Self::connect_unix(address).await,
        }
    }

    #[cfg(unix)]
    async fn connect_unix(address: &DaemonAddress) -> Result<Self, ClientError> {
        if address.path.is_empty() {
            return Err(ClientError::InvalidAddress(
                "missing Unix socket path".into(),
            ));
        }
        Ok(Self::Unix(BufReader::new(
            UnixStream::connect(&address.path).await?,
        )))
    }

    #[cfg(not(unix))]
    async fn connect_unix(_address: &DaemonAddress) -> Result<Self, ClientError> {
        Err(ClientError::InvalidAddress(
            "Unix sockets are unsupported".into(),
        ))
    }

    pub(super) async fn exchange(
        &mut self,
        request: &DaemonRequest,
    ) -> Result<DaemonResponse, ClientError> {
        match self {
            Self::Tcp(stream) => exchange(stream, request).await,
            #[cfg(unix)]
            Self::Unix(stream) => exchange(stream, request).await,
        }
    }
}

async fn exchange<S>(
    stream: &mut BufReader<S>,
    request: &DaemonRequest,
) -> Result<DaemonResponse, ClientError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut payload = serde_json::to_vec(request)?;
    payload.push(b'\n');
    stream.get_mut().write_all(&payload).await?;
    stream.get_mut().flush().await?;
    let mut line = String::new();
    if stream.read_line(&mut line).await? == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "daemon closed without response",
        )
        .into());
    }
    Ok(serde_json::from_str(&line)?)
}
