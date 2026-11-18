//! A finite registration exchange with a managed session's actual owner.
//! It does not pass through or start the collaboration daemon.

use cccc_core::HomeLayout;
use cccc_core::owned_mcp::{Binding, Registration};
#[cfg(unix)]
use cccc_core::owned_mcp::{REGISTRATION_API, RegistrationRequest, RegistrationResponse};
use std::io;

#[cfg(unix)]
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

/// Registration is idempotent for this process/binding; a partially exchanged
/// request is not retried here. The deadline includes connect, send and reply.
#[cfg(unix)]
pub async fn register(
    home: &HomeLayout,
    binding: &Binding,
    report: &Registration,
) -> io::Result<()> {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    report.validate()?;
    if report.instance != binding.instance || report.launch_id != binding.launch_id {
        return Err(invalid("MCP registration does not match its owned binding"));
    }
    let exchange = async {
        let socket = cccc_core::unix_socket_path::SocketPath::new(&binding.socket(home)?)?;
        let mut stream = UnixStream::connect(&socket.path).await?;
        drop(socket);
        let mut request = serde_json::to_vec(&RegistrationRequest::McpRegister {
            report: report.clone(),
        })
        .map_err(invalid)?;
        request.push(b'\n');
        stream.write_all(&request).await?;
        let mut frame = Vec::new();
        BufReader::new(stream)
            .take(MAX_RESPONSE_BYTES + 1)
            .read_until(b'\n', &mut frame)
            .await?;
        if frame.len() as u64 > MAX_RESPONSE_BYTES || frame.last() != Some(&b'\n') {
            return Err(invalid(
                "session owner registration response is incomplete or too large",
            ));
        }
        match serde_json::from_slice::<RegistrationResponse>(&frame).map_err(invalid)? {
            RegistrationResponse::McpRegistered {
                registration,
                instance,
                launch_id,
            } if registration == REGISTRATION_API
                && instance == binding.instance
                && launch_id == binding.launch_id =>
            {
                Ok(())
            }
            RegistrationResponse::McpRegistered { .. } => {
                Err(invalid("session owner confirmed another MCP binding"))
            }
            RegistrationResponse::Failed { error } => Err(io::Error::other(format!(
                "{}: {}",
                error.code, error.message
            ))),
        }
    };
    tokio::time::timeout(Duration::from_secs(3), exchange)
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "session owner registration timed out",
            )
        })?
}

#[cfg(not(unix))]
pub async fn register(
    _home: &HomeLayout,
    _binding: &Binding,
    _report: &Registration,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "owned MCP registration requires a Unix session owner",
    ))
}

#[cfg(unix)]
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
