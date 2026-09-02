use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader};
use std::net::IpAddr;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

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
    configure_process_group(&mut process);
    let mut child = process.spawn()?;
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
    Ok((ChildOwner::new(child), receiver))
}

fn spawn_output_reader(
    stream: impl std::io::Read + Send + 'static,
    sender: std::sync::mpsc::Sender<String>,
    suffix: &str,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name(format!("cccc-codex-app-{suffix}"))
        .spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                tracing::debug!(message = %line, "Codex app-server output");
                let _ = sender.send(line);
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

pub(super) struct ChildOwner {
    child: Mutex<Option<Child>>,
}

impl ChildOwner {
    fn new(child: Child) -> Self {
        Self {
            child: Mutex::new(Some(child)),
        }
    }

    pub(super) fn stop(&self) -> io::Result<()> {
        let Some(mut child) = self
            .child
            .lock()
            .map_err(|_| io::Error::other("Codex app-server child lock poisoned"))?
            .take()
        else {
            return Ok(());
        };
        if child.try_wait()?.is_none() {
            terminate_process_group(&mut child);
            if !wait_bounded(&mut child, STOP_TIMEOUT)? {
                kill_process_group(&mut child);
            }
        }
        let _ = child.wait();
        Ok(())
    }

    pub(super) fn running(&self) -> bool {
        self.child
            .lock()
            .ok()
            .and_then(|mut child| child.as_mut().map(|child| child.try_wait()))
            .is_some_and(|status| status.ok().flatten().is_none())
    }

    pub(super) fn id(&self) -> Option<u32> {
        self.child
            .lock()
            .ok()
            .and_then(|child| child.as_ref().map(Child::id))
    }
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        let child = self.child.get_mut().ok().and_then(Option::take);
        if let Some(mut child) = child {
            if child.try_wait().ok().flatten().is_none() {
                kill_process_group(&mut child);
            }
            let _ = child.wait();
        }
    }
}

fn wait_bounded(child: &mut Child, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_process_group(child: &mut Child) {
    signal_process_group(child, nix::sys::signal::Signal::SIGTERM);
}

#[cfg(unix)]
fn kill_process_group(child: &mut Child) {
    signal_process_group(child, nix::sys::signal::Signal::SIGKILL);
    let _ = child.kill();
}

#[cfg(unix)]
fn signal_process_group(child: &Child, signal: nix::sys::signal::Signal) {
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;
    if let Ok(group_id) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(group_id), signal);
    }
}

#[cfg(windows)]
fn terminate_process_group(child: &mut Child) {
    kill_process_group(child);
}

#[cfg(windows)]
fn kill_process_group(child: &mut Child) {
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

#[cfg(all(not(unix), not(windows)))]
fn terminate_process_group(child: &mut Child) {
    let _ = child.kill();
}

#[cfg(all(not(unix), not(windows)))]
fn kill_process_group(child: &mut Child) {
    let _ = child.kill();
}
