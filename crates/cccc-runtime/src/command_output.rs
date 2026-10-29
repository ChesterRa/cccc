//! Finite command execution. The invocation owns the child tree and all pipes;
//! use runtime sessions for interactive or persistent commands instead.
use crate::{CommandError, OwnedProcessTree};
use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub(crate) struct OwnedChild {
    pub(crate) child: Child,
    pub(crate) tree: OwnedProcessTree,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Terminate before reaping: an unreaped Unix leader pins the group ID.
        // On cancellation this also closes pipes inherited by owned children.
        if self.tree.terminate().is_ok() {
            let _ = self.child.wait();
        }
    }
}

/// Capture at most `limit` bytes per stream, draining the rest without retaining
/// it. Timeout covers input, output and exit together. Dropping the future ends
/// the owned command too. Descendants in the owned process group / Windows Job
/// are terminated when the leader exits; deliberately detached services are not
/// supported by this finite-command interface.
pub async fn capture_command(
    command: &mut Command,
    input: Option<&[u8]>,
    timeout: Duration,
    limit: usize,
) -> io::Result<CapturedOutput> {
    capture_command_with_start_evidence(command, input, timeout, limit)
        .await
        .map_err(CommandError::into_io_error)
}

/// The same finite capture with explicit process-start evidence. `NotStarted`
/// means admission was refused or `Command::spawn` failed; the requested
/// executable was not started. Resource registration, pipe setup, I/O and
/// timeout after a successful spawn all remain `Started`, even if their error
/// kind also occurs during spawn. Dropping a polled future has no returned
/// evidence and must not be treated as proof of non-submission.
pub async fn capture_command_with_start_evidence(
    command: &mut Command,
    input: Option<&[u8]>,
    timeout: Duration,
    limit: usize,
) -> Result<CapturedOutput, CommandError> {
    let deadline = tokio::time::Instant::now() + timeout;
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, tree) = OwnedProcessTree::spawn_registered(command, 0)?;
    let mut owned = OwnedChild { child, tree };
    let stdout = tokio::process::ChildStdout::from_std(
        owned
            .child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("command stdout unavailable"))
            .map_err(CommandError::Started)?,
    )
    .map_err(CommandError::Started)?;
    let stderr = tokio::process::ChildStderr::from_std(
        owned
            .child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("command stderr unavailable"))
            .map_err(CommandError::Started)?,
    )
    .map_err(CommandError::Started)?;
    let stdin = owned
        .child
        .stdin
        .take()
        .map(tokio::process::ChildStdin::from_std)
        .transpose()
        .map_err(CommandError::Started)?;
    let operation = async {
        let write = async move {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                stdin.write_all(input).await?;
            }
            Ok::<_, io::Error>(())
        };
        let wait = async {
            loop {
                if let Some(status) = owned.tree.try_wait(|| owned.child.try_wait())? {
                    return Ok::<_, io::Error>(status);
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        };
        let (_, (stdout, stdout_truncated), (stderr, stderr_truncated), status) = tokio::try_join!(
            write,
            read_bounded(stdout, limit),
            read_bounded(stderr, limit),
            wait
        )?;
        Ok(CapturedOutput {
            status,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        })
    };
    tokio::time::timeout_at(deadline, operation)
        .await
        .map_err(|_| {
            CommandError::Started(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("command timed out after {}s", timeout.as_secs_f64()),
            ))
        })?
        .map_err(CommandError::Started)
}

/// Synchronous ports use the same capture semantics without a permanent worker
/// or a second process implementation. Avoid nesting a Tokio runtime when a
/// synchronous handler is invoked from an asynchronous embedding.
pub fn capture_command_blocking(
    command: &mut Command,
    input: Option<&[u8]>,
    timeout: Duration,
    limit: usize,
) -> io::Result<CapturedOutput> {
    let mut run = || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(capture_command(command, input, timeout, limit))
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| match scope.spawn(run).join() {
            Ok(output) => output,
            Err(panic) => std::panic::resume_unwind(panic),
        })
    } else {
        run()
    }
}

async fn read_bounded(
    mut stream: impl AsyncRead + Unpin,
    limit: usize,
) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 8192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok((bytes, truncated));
        }
        let retained = count.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..retained]);
        truncated |= retained < count;
    }
}

#[cfg(all(test, unix))]
mod tests;
