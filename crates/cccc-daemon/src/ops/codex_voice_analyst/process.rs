use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader};
use std::net::IpAddr;
use std::path::Path;
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};
use url::Url;

mod child;
use cccc_runtime::OwnedProcessTree;
pub(super) use child::ChildOwner;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn spawn_app_server(
    command: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> io::Result<(ChildOwner, std::sync::mpsc::Receiver<String>)> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty Codex command"))?;
    let mut process = Command::new(program);
    process
        .args(args)
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut child, process_tree) = OwnedProcessTree::spawn(&mut process)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Codex app-server stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Codex app-server stderr is unavailable"))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    spawn_output_reader(stdout, sender.clone(), "stdout")?;
    spawn_output_reader(stderr, sender, "stderr")?;
    Ok((ChildOwner::new(child, process_tree), receiver))
}

pub(super) fn spawn_background(
    command: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    label: &'static str,
) -> io::Result<ChildOwner> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty managed command"))?;
    let mut process = Command::new(program);
    process
        .args(args)
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut child, process_tree) = OwnedProcessTree::spawn(&mut process)?;
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(stdout, label)?;
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(stderr, label)?;
    }
    Ok(ChildOwner::new(child, process_tree))
}

pub(super) fn spawn_piped(
    command: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
    label: &'static str,
) -> io::Result<(ChildOwner, ChildStdin, ChildStdout)> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty managed command"))?;
    let mut process = Command::new(program);
    process
        .args(args)
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (mut child, process_tree) = OwnedProcessTree::spawn(&mut process)?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("managed process stdin is unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("managed process stdout is unavailable"))?;
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(stderr, label)?;
    }
    Ok((ChildOwner::new(child, process_tree), stdin, stdout))
}

fn spawn_output_reader(
    stream: impl std::io::Read + Send + 'static,
    sender: std::sync::mpsc::Sender<String>,
    suffix: &str,
) -> io::Result<()> {
    let suffix = suffix.to_owned();
    std::thread::Builder::new()
        .name(format!("cccc-codex-app-{suffix}"))
        .spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                tracing::debug!(message = %line, source = %suffix, "managed Agent process output");
                let _ = sender.send(line);
            }
        })?;
    Ok(())
}

fn spawn_log_reader(
    stream: impl std::io::Read + Send + 'static,
    source: &'static str,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("cccc-managed-agent-{source}"))
        .spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                tracing::debug!(message = %line, source, "managed Agent process output");
            }
        })?;
    Ok(())
}

pub(super) async fn wait_for_endpoint(
    receiver: std::sync::mpsc::Receiver<String>,
) -> io::Result<String> {
    tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut recent = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(remaining.min(Duration::from_millis(200))) {
                Ok(line) => {
                    if let Some(endpoint) = parse_listening_endpoint(&line) {
                        validate_loopback_endpoint(&endpoint)?;
                        return Ok(endpoint);
                    }
                    recent.push(line);
                    if recent.len() > 8 {
                        recent.remove(0);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let detail = recent.join(" | ");
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            if detail.is_empty() {
                "Codex app-server did not publish a loopback endpoint".into()
            } else {
                format!("Codex app-server did not publish a loopback endpoint: {detail}")
            },
        ))
    })
    .await
    .map_err(|error| io::Error::other(format!("endpoint reader failed: {error}")))?
}

pub(super) fn parse_listening_endpoint(line: &str) -> Option<String> {
    line.split_once("listening on:")
        .map(|(_, value)| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(super) fn validate_loopback_endpoint(endpoint: &str) -> io::Result<()> {
    let parsed = Url::parse(endpoint).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid Codex app-server endpoint: {error}"),
        )
    })?;
    let loopback = parsed.scheme() == "ws"
        && parsed.host().is_some_and(|host| match host {
            url::Host::Ipv4(address) => IpAddr::V4(address).is_loopback(),
            url::Host::Ipv6(address) => IpAddr::V6(address).is_loopback(),
            url::Host::Domain(_) => false,
        })
        && parsed.port().is_some_and(|port| port > 0)
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none();
    if loopback {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Codex app-server requires an uncredentialed loopback ws endpoint",
        ))
    }
}
