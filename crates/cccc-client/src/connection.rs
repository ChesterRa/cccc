use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::ClientError;

#[cfg(unix)]
use tokio::net::UnixStream;

const CONNECTION_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(super) enum Connection {
    Tcp(BufReader<TcpStream>),
    #[cfg(unix)]
    Unix(BufReader<UnixStream>),
}

#[derive(Debug)]
pub struct DaemonStream {
    connection: Connection,
}

impl DaemonStream {
    pub(super) fn new(connection: Connection) -> Self {
        Self { connection }
    }
}

impl AsyncRead for DaemonStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match &mut self.get_mut().connection {
            Connection::Tcp(stream) => Pin::new(stream).poll_read(context, buffer),
            #[cfg(unix)]
            Connection::Unix(stream) => Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for DaemonStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match &mut self.get_mut().connection {
            Connection::Tcp(stream) => Pin::new(stream).poll_write(context, buffer),
            #[cfg(unix)]
            Connection::Unix(stream) => Pin::new(stream).poll_write(context, buffer),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match &mut self.get_mut().connection {
            Connection::Tcp(stream) => Pin::new(stream).poll_flush(context),
            #[cfg(unix)]
            Connection::Unix(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match &mut self.get_mut().connection {
            Connection::Tcp(stream) => Pin::new(stream).poll_shutdown(context),
            #[cfg(unix)]
            Connection::Unix(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
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
                Ok(Self::Tcp(BufReader::with_capacity(
                    CONNECTION_BUFFER_BYTES,
                    stream,
                )))
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
        Ok(Self::Unix(BufReader::with_capacity(
            CONNECTION_BUFFER_BYTES,
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
