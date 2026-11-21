//! Darwin birth and boot evidence. Missing native information stays unknown;
//! neither recovery nor watchdog signalling falls back to approximate `ps` time.

use super::guard_record::{DarwinIdentity, GroupEvidence, GroupRecord, Origin};
use crate::darwin_process::ProcessInfo;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, waitid};
use std::io::{self, Read};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub(super) struct ProcessView {
    origin: Origin,
    uid: u32,
}

impl ProcessView {
    pub(super) fn open() -> io::Result<Self> {
        Ok(Self {
            origin: boot_origin()?,
            uid: rustix::process::geteuid().as_raw(),
        })
    }

    pub(super) fn origin(&self) -> Origin {
        self.origin.clone()
    }

    /// The caller holds the child and serializes its own waits. As on Linux,
    /// embedders must not install an external or automatic child reaper.
    pub(super) fn capture_child(&self, pid: i32) -> io::Result<DarwinIdentity> {
        let child = Pid::from_raw(pid).ok_or_else(|| invalid("invalid child PID"))?;
        confirm_child(child)?;
        let info = ProcessInfo::read(pid as u32)?;
        if info.parent_pid != std::process::id()
            || info.group_id != pid as u32
            || info.uid != self.uid
        {
            return Err(invalid("child ownership or process group changed"));
        }
        confirm_child(child)?;
        let after = ProcessInfo::read(pid as u32)?;
        if info.pid != after.pid
            || info.parent_pid != after.parent_pid
            || info.group_id != after.group_id
            || info.uid != after.uid
            || info.started_seconds != after.started_seconds
            || info.started_microseconds != after.started_microseconds
        {
            return Err(invalid("child identity changed during capture"));
        }
        Ok(DarwinIdentity {
            origin: self.origin.clone(),
            pid: info.pid,
            uid: info.uid,
            started_seconds: info.started_seconds,
            started_microseconds: info.started_microseconds,
        })
    }

    pub(super) fn signal_group(&self, group: &GroupRecord, signal: Signal) -> io::Result<()> {
        let GroupEvidence::DarwinCaptured(identity) = &group.evidence else {
            return Err(invalid("group has no Darwin birth evidence"));
        };
        if group.pgid <= 1 {
            return Err(invalid("refusing a system process group"));
        }
        let pgid = Pid::from_raw(group.pgid).ok_or_else(|| invalid("invalid process group"))?;
        if identity.origin != self.origin
            || group.origin.as_ref() != Some(&self.origin)
            || identity.pid != group.pgid as u32
            || identity.uid != self.uid
            || nix::unistd::getpgrp().as_raw() == group.pgid
        {
            return Err(invalid("process origin or ownership is unconfirmed"));
        }
        let current = ProcessInfo::read(identity.pid)?;
        if current.uid != identity.uid
            || current.group_id != identity.pid
            || current.started_seconds != identity.started_seconds
            || current.started_microseconds != identity.started_microseconds
        {
            return Err(invalid("process group leader birth or group changed"));
        }
        // Darwin has no pidfd group signal. This is the explicit native
        // check-to-signal window; the same evidence is checked again for KILL.
        rustix::process::kill_process_group(pgid, signal).map_err(Into::into)
    }
}

fn confirm_child(pid: Pid) -> io::Result<()> {
    waitid(
        WaitId::Pid(pid),
        WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
    )?;
    Ok(())
}

fn boot_origin() -> io::Result<Origin> {
    static BOOT: OnceLock<Origin> = OnceLock::new();
    if let Some(origin) = BOOT.get() {
        return Ok(origin.clone());
    }
    let mut child = crate::background_command("/usr/sbin/sysctl")
        .args(["-n", "kern.bootsessionuuid"])
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "boot identity query timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        return Err(invalid("boot identity query failed"));
    }
    let mut output = String::new();
    child
        .stdout
        .take()
        .ok_or_else(|| invalid("boot identity output is missing"))?
        .take(129)
        .read_to_string(&mut output)?;
    if output.len() > 128 {
        return Err(invalid("boot identity output exceeds its bound"));
    }
    let id = uuid::Uuid::parse_str(output.trim()).map_err(|_| invalid("boot UUID is invalid"))?;
    if id.is_nil() {
        return Err(invalid("boot UUID is empty"));
    }
    let origin = Origin::Darwin {
        boot_id: id.hyphenated().to_string(),
    };
    let _ = BOOT.set(origin.clone());
    Ok(origin)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    struct TestChild(std::process::Child);
    impl Drop for TestChild {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    #[test]
    fn actual_child_birth_rejects_a_changed_identity_before_signalling() {
        let view = ProcessView::open().expect("native boot UUID");
        assert!(view.origin.valid());
        let mut child = TestChild(
            crate::background_command("/bin/sleep")
                .arg("30")
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("owned child"),
        );
        let captured = view
            .capture_child(child.0.id() as i32)
            .expect("native birth");
        let record = GroupRecord {
            id: uuid::Uuid::new_v4(),
            source_id: None,
            pgid: child.0.id() as i32,
            spawned_at: super::super::guard::now(),
            observed_at: super::super::guard::now(),
            origin: Some(view.origin()),
            evidence: GroupEvidence::DarwinCaptured(captured.clone()),
        };
        let mut wrong = record.clone();
        let GroupEvidence::DarwinCaptured(identity) = &mut wrong.evidence else {
            unreachable!()
        };
        identity.started_microseconds = (identity.started_microseconds + 1) % 1_000_000;
        assert!(view.signal_group(&wrong, Signal::TERM).is_err());
        assert!(child.0.try_wait().expect("still owned").is_none());
        view.signal_group(&record, Signal::KILL)
            .expect("matching native child");
        child.0.wait().expect("reap owned child");
    }
}
