// Fixture cleanup uses a pinned process, never an unowned numeric process group.
#[cfg(target_os = "linux")]
pub(super) struct FixtureProcess(std::os::fd::OwnedFd);

#[cfg(target_os = "linux")]
impl FixtureProcess {
    pub(super) fn pin(pid: i32, parent: u32) -> std::io::Result<Self> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use rustix::process::{Pid, PidfdFlags, pidfd_open};
        use std::os::fd::AsRawFd;

        let pid_value = Pid::from_raw(pid).ok_or_else(|| std::io::Error::other("invalid PID"))?;
        let fd = pidfd_open(pid_value, PidfdFlags::empty())?;
        let fdinfo = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd()))?;
        let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
        let named_pid = proc_status_number(&fdinfo, "Pid:");
        let observed_parent = proc_status_number(&status, "PPid:");
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if named_pid != Some(pid as u32)
            || observed_parent != Some(parent)
            || poll(&mut fds, Some(&timeout))? != 0
        {
            return Err(std::io::Error::other("fixture identity is unconfirmed"));
        }
        Ok(Self(fd))
    }

    pub(super) fn kill(&self) -> std::io::Result<()> {
        rustix::process::pidfd_send_signal(&self.0, rustix::process::Signal::KILL)?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

#[cfg(target_os = "linux")]
fn proc_status_number(status: &str, key: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key)?.trim().parse().ok())
}

#[cfg(target_os = "linux")]
pub(super) fn fixture_pidfds_available() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    let same_view = status
        .lines()
        .find_map(|line| line.strip_prefix("NSpid:"))
        .is_some_and(|pids| {
            let mut pids = pids.split_whitespace();
            pids.next().and_then(|pid| pid.parse::<u32>().ok()) == Some(std::process::id())
                && pids.next().is_none()
        });
    let self_pid = rustix::process::Pid::from_raw(std::process::id() as i32);
    same_view
        && self_pid.is_some_and(|pid| {
            rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).is_ok()
        })
}

// On platforms without pidfds, the fixture has a finite lifetime and its
// helper remains a directly owned Child. Cleanup never signals a numeric PGID.
#[cfg(not(target_os = "linux"))]
pub(super) struct FixtureProcess;

#[cfg(not(target_os = "linux"))]
impl FixtureProcess {
    pub(super) fn pin(_pid: i32, _parent: u32) -> std::io::Result<Self> {
        Ok(Self)
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn fixture_pidfds_available() -> bool {
    true
}
