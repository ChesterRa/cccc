//! Native process evidence shared by the Mac guard and its higher-level hosts.
//! A failed libproc read is unknown unless the kernel independently confirms exit.

use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub group_id: u32,
    pub uid: u32,
    pub started_seconds: u64,
    pub started_microseconds: u32,
    pub zombie: bool,
}

impl ProcessInfo {
    /// Include unreaped children: a guard may still own their process group.
    /// A host-liveness consumer must explicitly exclude `zombie` results.
    pub fn read(pid: u32) -> io::Result<Self> {
        use libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
        use rustix::process::{Pid, test_kill_process};
        let native_pid = i32::try_from(pid).map_err(io::Error::other)?;
        let native = Pid::from_raw(native_pid).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid native process id")
        })?;
        let info = pidinfo::<BSDInfo>(native_pid, 1).map_err(|_| {
            // libproc exposes a formatted string, not a typed errno. Do not
            // classify an inaccessible or missing row by its English text.
            match test_kill_process(native) {
                Err(rustix::io::Errno::SRCH) => {
                    io::Error::new(io::ErrorKind::NotFound, "native process has exited")
                }
                Err(rustix::io::Errno::PERM) => io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "native process is inaccessible",
                ),
                _ => io::Error::other("native process identity is unavailable"),
            }
        })?;
        let micros = u32::try_from(info.pbi_start_tvusec).map_err(io::Error::other)?;
        if info.pbi_pid != pid || info.pbi_start_tvsec == 0 || micros >= 1_000_000 {
            return Err(io::Error::other("native process identity is invalid"));
        }
        Ok(Self {
            pid,
            parent_pid: info.pbi_ppid,
            group_id: info.pbi_pgid,
            uid: info.pbi_uid,
            started_seconds: info.pbi_start_tvsec,
            started_microseconds: micros,
            zombie: info.pbi_status == nix::libc::SZOMB,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    #[test]
    fn native_birth_and_parent_stay_available_until_an_owned_child_is_reaped() {
        let mut child = crate::background_command("/bin/sh")
            .args(["-c", "exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("owned child");
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        let info = loop {
            let info = ProcessInfo::read(pid).expect("native child identity");
            assert_eq!(info.parent_pid, std::process::id());
            if info.zombie {
                break info;
            }
            assert!(Instant::now() < deadline, "child did not exit");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(ProcessInfo::read(pid).expect("unreaped birth"), info);
        child.wait().expect("reap owned child");
        if let Ok(reused) = ProcessInfo::read(pid) {
            assert_ne!(
                (reused.started_seconds, reused.started_microseconds),
                (info.started_seconds, info.started_microseconds),
                "a reused PID is a different birth"
            );
        }
    }
}
