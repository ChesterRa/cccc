use cccc_core::{HomeLayout, cloudflared, fs};
use reqwest::blocking::Client;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct TrackedProcess {
    pid: u32,
    executable: PathBuf,
}

#[derive(Debug)]
pub(super) struct RuntimeError {
    pub code: &'static str,
    pub message: String,
}

impl RuntimeError {
    fn network(message: impl Into<String>) -> Self {
        Self {
            code: "membership_network",
            message: message.into(),
        }
    }

    fn process(message: impl Into<String>) -> Self {
        Self {
            code: "membership_subprocess",
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Status {
    pub running: bool,
}

fn pid_path(home: &HomeLayout) -> PathBuf {
    cloudflared::install_dir(home).join("cloudflared.pid")
}

fn token_path(home: &HomeLayout) -> PathBuf {
    cloudflared::install_dir(home).join("cloudflared.token")
}

fn log_path(home: &HomeLayout) -> PathBuf {
    cloudflared::install_dir(home).join("cloudflared.log")
}

fn remove_if_present(path: &Path) -> Result<(), RuntimeError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RuntimeError::process(error.to_string())),
    }
}

fn tracked_process(home: &HomeLayout) -> Result<Option<TrackedProcess>, RuntimeError> {
    let path = pid_path(home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(RuntimeError::process(error.to_string())),
    };
    let trimmed = text.trim();
    let (pid, executable) = if trimmed.starts_with('{') {
        let marker: serde_json::Value = serde_json::from_str(trimmed).map_err(|_| {
            RuntimeError::process(
                "cloudflared PID marker is malformed; refusing to report it stopped",
            )
        })?;
        let pid = marker.get("pid").and_then(serde_json::Value::as_u64);
        let executable = marker
            .get("executable")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty());
        let (Some(pid), Some(executable)) = (pid, executable) else {
            return Err(RuntimeError::process(
                "cloudflared PID marker is malformed; refusing to report it stopped",
            ));
        };
        let pid = u32::try_from(pid).map_err(|_| {
            RuntimeError::process(
                "cloudflared PID marker is malformed; refusing to report it stopped",
            )
        })?;
        (pid, canonical_executable(Path::new(executable))?)
    } else {
        let pid = trimmed.parse::<u32>().map_err(|_| {
            RuntimeError::process(
                "cloudflared PID marker is malformed; refusing to report it stopped",
            )
        })?;
        (pid, canonical_executable(&cloudflared::binary_path(home))?)
    };
    if pid == 0 {
        return Err(RuntimeError::process(
            "cloudflared PID marker is invalid; refusing to report it stopped",
        ));
    }
    Ok(Some(TrackedProcess { pid, executable }))
}

fn canonical_executable(path: &Path) -> Result<PathBuf, RuntimeError> {
    std::fs::canonicalize(path).map_err(|error| {
        RuntimeError::process(format!(
            "tracked cloudflared executable {} is unavailable: {error}",
            path.display()
        ))
    })
}

#[cfg(unix)]
pub(super) fn process_is_alive(pid: u32) -> bool {
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        && stat
            .rsplit_once(") ")
            .and_then(|(_, tail)| tail.split_whitespace().next())
            == Some("Z")
    {
        return false;
    }
    #[cfg(not(target_os = "linux"))]
    if Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .trim_start()
                    .starts_with('Z')
        })
    {
        return false;
    }
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    match kill(Pid::from_raw(pid), None) {
        Ok(()) | Err(Errno::EPERM) => true,
        Err(Errno::ESRCH) => false,
        Err(_) => true,
    }
}

#[cfg(windows)]
pub(super) fn process_is_alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && !String::from_utf8_lossy(&output.stdout).contains("No tasks are running")
                && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
        })
}

#[cfg(not(any(unix, windows)))]
pub(super) fn process_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn process_executable(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Ok(path) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        return std::fs::canonicalize(path).ok();
    }
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!path.is_empty()).then_some(path)
        })
        .and_then(|path| std::fs::canonicalize(path).ok())
}

#[cfg(windows)]
fn process_executable(pid: u32) -> Option<PathBuf> {
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').ExecutablePath"),
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!path.is_empty()).then_some(path)
        })
        .and_then(|path| std::fs::canonicalize(path).ok())
}

#[cfg(not(any(unix, windows)))]
fn process_executable(_pid: u32) -> Option<PathBuf> {
    None
}

fn process_matches_executable(pid: u32, expected: &Path) -> bool {
    process_executable(pid).is_some_and(|actual| actual == expected)
}

fn retire_tracking(home: &HomeLayout) -> Result<(), RuntimeError> {
    remove_if_present(&pid_path(home))?;
    remove_if_present(&token_path(home))
}

pub(super) fn status(home: &HomeLayout) -> Status {
    let Ok(Some(tracked)) = tracked_process(home) else {
        return Status { running: false };
    };
    let pid = tracked.pid;
    if !process_is_alive(pid) {
        let _ = retire_tracking(home);
        return Status { running: false };
    }
    Status {
        running: process_matches_executable(pid, &tracked.executable),
    }
}

pub(super) fn ensure(
    home: &HomeLayout,
    upgrade: bool,
) -> Result<cloudflared::Inspection, RuntimeError> {
    let current =
        cloudflared::inspect(home).map_err(|error| RuntimeError::process(error.to_string()))?;
    if current.matches_pin {
        return Ok(current);
    }
    if current.installed && !upgrade {
        return Err(RuntimeError::process(
            "installed cloudflared is not the pinned release; run `cccc reach install` to upgrade",
        ));
    }
    let (system, machine) = cloudflared::current_platform();
    let artifact = cloudflared::artifact_for(system, machine).ok_or_else(|| {
        RuntimeError::process(format!(
            "cloudflared is not provided for {system}/{machine} in this release"
        ))
    })?;
    let url = cloudflared::download_url(&artifact);
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| RuntimeError::network(error.to_string()))?;
    let mut response = client
        .get(&url)
        .header("User-Agent", "cccc-cloudflared")
        .send()
        .map_err(|error| {
            RuntimeError::network(format!("failed to download cloudflared: {error}"))
        })?;
    if !response.status().is_success() {
        return Err(RuntimeError::network(format!(
            "failed to download cloudflared: HTTP {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > cloudflared::MAX_DOWNLOAD_BYTES as u64)
    {
        return Err(RuntimeError::process(
            "cloudflared download exceeded size limit",
        ));
    }
    let mut payload = Vec::new();
    response
        .by_ref()
        .take((cloudflared::MAX_DOWNLOAD_BYTES + 1) as u64)
        .read_to_end(&mut payload)
        .map_err(|error| RuntimeError::network(error.to_string()))?;
    if payload.len() > cloudflared::MAX_DOWNLOAD_BYTES {
        return Err(RuntimeError::process(
            "cloudflared download exceeded size limit",
        ));
    }
    cloudflared::install_from_bytes(home, &payload, true)
        .map_err(|error| RuntimeError::process(error.to_string()))
}

fn write_token(home: &HomeLayout, token: &str) -> Result<PathBuf, RuntimeError> {
    let path = token_path(home);
    let mut bytes = token.as_bytes().to_vec();
    bytes.push(b'\n');
    fs::atomic_write(&path, &bytes).map_err(|error| RuntimeError::process(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| RuntimeError::process(error.to_string()))?;
    }
    Ok(path)
}

pub(super) fn start(home: &HomeLayout, tunnel_token: &str) -> Result<Status, RuntimeError> {
    let token = tunnel_token.trim();
    if token.is_empty() {
        return Err(RuntimeError::process("missing tunnel token"));
    }
    stop(home)?;
    let installed =
        cloudflared::inspect(home).map_err(|error| RuntimeError::process(error.to_string()))?;
    if !installed.matches_pin {
        return Err(RuntimeError::process("pinned cloudflared is not installed"));
    }
    let binary = installed
        .path
        .ok_or_else(|| RuntimeError::process("pinned cloudflared is not installed"))?;
    let token_file = write_token(home, token)?;
    start_command(
        home,
        &[
            binary.to_string_lossy().into_owned(),
            "tunnel".into(),
            "--no-autoupdate".into(),
            "run".into(),
            "--token-file".into(),
            token_file.to_string_lossy().into_owned(),
        ],
    )
}

fn start_command(home: &HomeLayout, argv: &[String]) -> Result<Status, RuntimeError> {
    let Some(program) = argv.first() else {
        return Err(RuntimeError::process("cloudflared command is empty"));
    };
    std::fs::create_dir_all(cloudflared::install_dir(home))
        .map_err(|error| RuntimeError::process(error.to_string()))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(home))
        .map_err(|error| RuntimeError::process(error.to_string()))?;
    let stderr = log
        .try_clone()
        .map_err(|error| RuntimeError::process(error.to_string()))?;
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = remove_if_present(&token_path(home));
            return Err(RuntimeError::process(format!(
                "failed to start cloudflared: {error}"
            )));
        }
    };
    std::thread::sleep(Duration::from_millis(100));
    if let Some(exit) = child
        .try_wait()
        .map_err(|error| RuntimeError::process(error.to_string()))?
    {
        let _ = remove_if_present(&token_path(home));
        return Err(RuntimeError::process(format!(
            "cloudflared exited during startup ({exit}); see {}",
            log_path(home).display()
        )));
    }
    let pid = child.id();
    let executable = match canonical_executable(Path::new(program)) {
        Ok(executable) => executable,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = remove_if_present(&token_path(home));
            return Err(error);
        }
    };
    if let Err(error) = fs::write_json(
        &pid_path(home),
        &serde_json::json!({"schema":1,"pid":pid,"executable":executable}),
    ) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = remove_if_present(&token_path(home));
        return Err(RuntimeError::process(format!(
            "failed to track cloudflared process {pid}: {error}"
        )));
    }
    let tracked_pid_path = pid_path(home);
    let tracked_token_path = token_path(home);
    std::thread::spawn(move || {
        let _ = child.wait();
        let still_owned = std::fs::read_to_string(&tracked_pid_path)
            .ok()
            .and_then(|tracked| serde_json::from_str::<serde_json::Value>(&tracked).ok())
            .and_then(|tracked| tracked.get("pid").and_then(serde_json::Value::as_u64))
            == Some(u64::from(pid));
        if still_owned {
            let _ = std::fs::remove_file(tracked_pid_path);
            let _ = std::fs::remove_file(tracked_token_path);
        }
    });
    Ok(Status { running: true })
}

#[cfg(unix)]
fn signal_stop(pid: u32, force: bool) -> Result<(), RuntimeError> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let signal = if force {
        Signal::SIGKILL
    } else {
        Signal::SIGTERM
    };
    let pid = i32::try_from(pid)
        .map_err(|_| RuntimeError::process("tracked cloudflared PID is out of range"))?;
    match kill(Pid::from_raw(pid), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(RuntimeError::process(format!(
            "failed to stop cloudflared process {pid}: {error}"
        ))),
    }
}

#[cfg(windows)]
fn signal_stop(pid: u32, force: bool) -> Result<(), RuntimeError> {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T"]);
    if force {
        command.arg("/F");
    }
    let output = command
        .output()
        .map_err(|error| RuntimeError::process(error.to_string()))?;
    if output.status.success() || !process_is_alive(pid) {
        Ok(())
    } else {
        Err(RuntimeError::process(format!(
            "failed to stop cloudflared process {pid}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(not(any(unix, windows)))]
fn signal_stop(pid: u32, _force: bool) -> Result<(), RuntimeError> {
    Err(RuntimeError::process(format!(
        "this platform cannot safely stop cloudflared process {pid}"
    )))
}

fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_is_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !process_is_alive(pid)
}

pub(super) fn stop(home: &HomeLayout) -> Result<(), RuntimeError> {
    let Some(tracked) = tracked_process(home)? else {
        remove_if_present(&token_path(home))?;
        return Ok(());
    };
    let pid = tracked.pid;
    if !process_is_alive(pid) {
        return retire_tracking(home);
    }
    if !process_matches_executable(pid, &tracked.executable) {
        return Err(RuntimeError::process(format!(
            "tracked PID {pid} is not the tracked cloudflared executable; refusing to terminate it"
        )));
    }
    signal_stop(pid, false)?;
    if !wait_for_exit(pid, Duration::from_secs(5)) {
        signal_stop(pid, true)?;
        if !wait_for_exit(pid, Duration::from_secs(2)) {
            return Err(RuntimeError::process(format!(
                "cloudflared process {pid} did not exit"
            )));
        }
    }
    retire_tracking(home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn command_start_keeps_the_token_out_of_argv_and_stop_reaps_it() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        let helper = cloudflared::install_dir(&home).join("cloudflared-test-helper");
        std::fs::create_dir_all(helper.parent().expect("parent")).expect("dir");
        symlink("/bin/sleep", &helper).expect("symlink");
        write_token(&home, "secret-token").expect("token");
        let started = start_command(&home, &[helper.to_string_lossy().into_owned(), "30".into()])
            .expect("start");
        assert!(started.running);
        assert_eq!(
            std::fs::read_to_string(token_path(&home))
                .expect("token")
                .trim(),
            "secret-token"
        );
        stop(&home).expect("stop");
        assert!(!status(&home).running);
        assert!(!pid_path(&home).exists());
        assert!(!token_path(&home).exists());
    }

    #[cfg(unix)]
    #[test]
    fn stop_refuses_a_reused_non_cloudflared_pid() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        let decoy = temp.path().join("cloudflared-decoy");
        symlink("/bin/sleep", &decoy).expect("decoy binary");
        let mut child = Command::new(&decoy).arg("30").spawn().expect("sleep");
        std::fs::create_dir_all(cloudflared::install_dir(&home)).expect("dir");
        symlink("/bin/false", cloudflared::binary_path(&home)).expect("managed binary");
        std::fs::write(pid_path(&home), child.id().to_string()).expect("pid");
        let error = stop(&home).expect_err("must refuse");
        assert!(error.message.contains("tracked cloudflared executable"));
        child.kill().expect("kill");
        child.wait().expect("wait");
    }

    #[cfg(unix)]
    #[test]
    fn start_reaps_the_child_when_pid_tracking_cannot_be_written() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        let helper = cloudflared::install_dir(&home).join("cloudflared-pid-fixture");
        std::fs::create_dir_all(helper.parent().expect("parent")).expect("dir");
        symlink("/bin/sleep", &helper).expect("helper");
        std::fs::create_dir_all(pid_path(&home)).expect("blocking pid directory");
        write_token(&home, "secret-token").expect("token");

        let error = start_command(&home, &[helper.to_string_lossy().into_owned(), "30".into()])
            .expect_err("pid tracking must fail");

        assert!(error.message.contains("failed to track cloudflared"));
        let pid = error
            .message
            .strip_prefix("failed to track cloudflared process ")
            .and_then(|message| message.split_once(':').map(|(pid, _)| pid))
            .expect("tracked pid")
            .parse::<u32>()
            .expect("pid");
        assert!(wait_for_exit(pid, Duration::from_secs(2)));
        assert!(!token_path(&home).exists());
    }
}
