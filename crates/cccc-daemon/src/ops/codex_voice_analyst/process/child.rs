use std::io;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub(in crate::ops::codex_voice_analyst) struct ChildOwner {
    child: Mutex<Option<Child>>,
}

impl ChildOwner {
    pub(in crate::ops::codex_voice_analyst) fn new(child: Child) -> Self {
        Self {
            child: Mutex::new(Some(child)),
        }
    }

    pub(in crate::ops::codex_voice_analyst) fn stop(&self) -> io::Result<()> {
        let Some(mut child) = self
            .child
            .lock()
            .map_err(|_| io::Error::other("managed Agent child lock poisoned"))?
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

    pub(in crate::ops::codex_voice_analyst) fn running(&self) -> bool {
        self.child
            .lock()
            .ok()
            .and_then(|mut child| child.as_mut().map(|child| child.try_wait()))
            .is_some_and(|status| status.ok().flatten().is_none())
    }

    pub(in crate::ops::codex_voice_analyst) fn id(&self) -> Option<u32> {
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
pub(super) fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
pub(super) fn configure_process_group(_command: &mut Command) {}

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
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
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
