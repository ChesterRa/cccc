//! Linux identity reads used by owned-process recovery.
//!
//! A PID from `/proc` is useful for a syscall only when both are in the same
//! PID namespace. Keep the checked procfs mount open for all subsequent reads;
//! a missing or inaccessible witness is not an empty process list.

use super::guard_record::{
    CapturedIdentity, GroupEvidence, GroupRecord, IdentityStrength, Origin, UnavailableReason,
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{AtFlags, Dir, Mode, OFlags, fstat, fstatfs, openat, statat};
use rustix::process::{
    Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, pidfd_open, pidfd_send_signal, waitid,
};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsFd, AsRawFd};

enum Leader {
    Pinned(std::os::fd::OwnedFd),
    Numeric,
}

const PROC_SUPER_MAGIC: u64 = 0x9fa0;
const PID_FS_MAGIC: u64 = 0x5049_4446;
const ANON_INODE_FS_MAGIC: u64 = 0x0904_1934;
const MAX_PROC_READ: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProcessStat {
    pub(super) pid: i32,
    pub(super) group: i32,
    pub(super) start_ticks: u64,
    pub(super) state: char,
}

pub(super) struct ProcView {
    root: File,
}

impl ProcView {
    pub(super) fn open() -> io::Result<Self> {
        Self::open_path(std::path::Path::new("/proc"))
    }

    fn open_path(path: &std::path::Path) -> io::Result<Self> {
        let root = File::open(path)?;
        if fstatfs(&root)?.f_type as u64 != PROC_SUPER_MAGIC {
            return Err(invalid("process view is not procfs"));
        }
        let view = Self { root };
        check_local_view(&view.read("self/status")?, std::process::id())?;
        Ok(view)
    }

    pub(super) fn origin(&self) -> io::Result<Origin> {
        let boot_id = parse_boot_id(&self.read("sys/kernel/random/boot_id")?)?;
        let pid_namespace = self.namespace_inode("thread-self/ns/pid")?;
        let time_namespace = match self.namespace_inode("thread-self/ns/time") {
            Ok(inode) => Some(inode),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Origin {
            boot_id,
            pid_namespace,
            time_namespace,
        })
    }

    pub(super) fn process(&self, pid: i32) -> io::Result<ProcessStat> {
        if pid <= 0 {
            return Err(invalid("invalid process id"));
        }
        let stat = parse_process_stat(&self.read_bytes(&format!("{pid}/stat"))?)?;
        if stat.pid != pid {
            return Err(invalid("process row does not match requested pid"));
        }
        Ok(stat)
    }

    #[cfg(test)]
    pub(super) fn parent_pid(&self, pid: i32) -> io::Result<i32> {
        let status = self.read(&format!("{pid}/status"))?;
        let mut fields = status.lines().filter_map(|line| line.strip_prefix("PPid:"));
        let parent = fields
            .next()
            .and_then(|value| value.trim().parse::<i32>().ok())
            .filter(|parent| *parent > 0)
            .ok_or_else(|| invalid("fixture parent is not verifiable"))?;
        if fields.next().is_some() {
            return Err(invalid("ambiguous fixture parent"));
        }
        Ok(parent)
    }

    pub(super) fn process_ids(&self) -> io::Result<Vec<i32>> {
        let mut dir = Dir::read_from(&self.root)?;
        let mut result = Vec::new();
        while let Some(entry) = dir.read() {
            let entry = entry?;
            let Ok(name) = entry.file_name().to_str() else {
                continue;
            };
            if !name.is_empty()
                && name.bytes().all(|byte| byte.is_ascii_digit())
                && let Ok(pid) = name.parse::<i32>()
                && pid > 0
            {
                result.push(pid);
            }
        }
        Ok(result)
    }

    /// Check the ignored-SIGCHLD tripwire. This snapshot does not reveal
    /// SA_NOCLDWAIT and is not a proof that a disposition cannot change.
    pub(super) fn check_child_disposition(&self) -> io::Result<()> {
        check_sigchld_not_ignored(&self.read("self/status")?)
    }

    /// Capture evidence while the caller still holds the child handle and
    /// serializes all of its own waits. The caller must also ensure that no
    /// wildcard reaper or auto-reap SIGCHLD disposition can run from spawn
    /// until this method returns. The disposition snapshots are tripwires,
    /// not a replacement for that whole-window precondition.
    pub(super) fn capture_child(&self, pid: i32) -> io::Result<CapturedIdentity> {
        let child = Pid::from_raw(pid).ok_or_else(|| invalid("invalid child pid"))?;
        self.check_child_disposition()?;
        let origin = self.origin()?;
        confirm_unreaped_child(child)?;

        let (pidfd, unavailable) = match pidfd_open(child, PidfdFlags::empty()) {
            Ok(fd) => (Some(fd), None),
            Err(error) if error == rustix::io::Errno::NOSYS => {
                (None, Some(UnavailableReason::PidfdUnavailable))
            }
            Err(error) if error == rustix::io::Errno::PERM => {
                (None, Some(UnavailableReason::PidfdDenied))
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(fd) = &pidfd
            && self.pidfd_process(fd)? != pid
        {
            return Err(invalid("pidfd does not match child"));
        }
        let stat = self.process(pid)?;
        if stat.group != pid {
            return Err(invalid("child is not in its recorded process group"));
        }
        confirm_unreaped_child(child)?;
        if let Some(fd) = &pidfd
            && self.pidfd_process(fd)? != pid
        {
            return Err(invalid("pidfd changed during child capture"));
        }
        self.check_child_disposition()?;
        if self.origin()? != origin {
            return Err(invalid("process origin changed during child capture"));
        }

        let strength = match (&pidfd, unavailable) {
            (Some(fd), None) => classify_pidfd(fd)?,
            (None, Some(reason)) => IdentityStrength::Approximate { reason },
            _ => return Err(invalid("inconsistent pidfd capture")),
        };
        Ok(CapturedIdentity {
            origin,
            start_ticks: stat.start_ticks,
            strength,
        })
    }

    pub(super) fn pidfd_process(&self, fd: impl AsFd) -> io::Result<i32> {
        // Keep the borrowed descriptor alive for the procfs read. A bare fd
        // number could otherwise be closed and reused by another thread.
        let fd = fd.as_fd();
        parse_pidfd_pid(&self.read(&format!("self/fdinfo/{}", fd.as_raw_fd()))?)
    }

    /// Signal only members observed in the verified leader's group. A pidfd
    /// pins each selected process; a numeric fallback is limited to an already
    /// approximate record whose leader pidfd is unavailable for this target.
    pub(super) fn signal_group(&self, group: &GroupRecord, signal: Signal) -> io::Result<()> {
        let pgid = Pid::from_raw(group.pgid).ok_or_else(|| invalid("invalid group"))?;
        // A process group outside this PID namespace is reported as 0. Unlike
        // rustix's nonzero Pid wrapper, nix can represent that valid result.
        if nix::unistd::getpgrp().as_raw() == group.pgid {
            return Err(invalid("refusing to signal the caller's group"));
        }
        let leader = self.verified_leader(group)?;
        match leader {
            Leader::Numeric => {
                // This is the explicitly accepted numeric check-to-signal
                // window; it is never used to conceal a failed strong check.
                rustix::process::kill_process_group(pgid, signal).map_err(Into::into)
            }
            Leader::Pinned(fd) => self.signal_members(group.pgid, &fd, signal),
        }
    }

    fn verified_leader(&self, group: &GroupRecord) -> io::Result<Leader> {
        let GroupEvidence::Captured(identity) = &group.evidence else {
            return Err(invalid("group identity was not captured"));
        };
        if group.origin.as_ref() != Some(&identity.origin) || self.origin()? != identity.origin {
            return Err(invalid("group origin differs from the current view"));
        }
        let pid = Pid::from_raw(group.pgid).ok_or_else(|| invalid("invalid group"))?;
        let fd = match pidfd_open(pid, PidfdFlags::empty()) {
            Ok(fd) => Some(fd),
            Err(error)
                if matches!(identity.strength, IdentityStrength::Approximate { .. })
                    && (error == rustix::io::Errno::NOSYS || error == rustix::io::Errno::PERM) =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(fd) = &fd {
            if self.pidfd_process(fd)? != group.pgid {
                return Err(invalid("leader fd does not match the recorded pid"));
            }
            if let IdentityStrength::Pidfs { inode } = identity.strength {
                if !cfg!(target_pointer_width = "64") || fstatfs(fd)?.f_type as u64 != PID_FS_MAGIC
                {
                    return Err(invalid("recorded pidfs identity cannot be checked"));
                }
                if fstat(fd)?.st_ino != inode {
                    return Err(invalid("recorded pidfs identity does not match"));
                }
            }
        }
        let stat = self.process(group.pgid)?;
        if stat.start_ticks != identity.start_ticks {
            return Err(invalid("leader start time does not match"));
        }
        if let Some(fd) = fd {
            if self.pidfd_process(&fd)? != group.pgid {
                return Err(invalid("leader changed during identity check"));
            }
            require_running_pidfd(&fd)?;
            Ok(Leader::Pinned(fd))
        } else {
            if stat.group != group.pgid {
                return Err(invalid("numeric leader is outside the group"));
            }
            Ok(Leader::Numeric)
        }
    }

    fn signal_members(&self, pgid: i32, leader: impl AsFd, signal: Signal) -> io::Result<()> {
        self.signal_members_in(pgid, leader, signal, self.process_ids()?)
    }

    fn signal_members_in(
        &self,
        pgid: i32,
        leader: impl AsFd,
        signal: Signal,
        mut pids: Vec<i32>,
    ) -> io::Result<()> {
        // Keep the leader attached as long as possible while verifying members.
        pids.sort_unstable_by_key(|pid| (*pid == pgid, *pid));
        let mut uncertain = false;
        for pid in pids {
            let before = match self.process(pid) {
                Ok(stat) => stat,
                Err(error) if process_gone(&error) => continue,
                Err(_) => {
                    uncertain = true;
                    continue;
                }
            };
            if before.group != pgid {
                continue;
            }
            if pid == std::process::id() as i32 {
                uncertain = true;
                continue;
            }
            let Some(target) = Pid::from_raw(pid) else {
                uncertain = true;
                continue;
            };
            let fd = match pidfd_open(target, PidfdFlags::empty()) {
                Ok(fd) => fd,
                Err(rustix::io::Errno::SRCH) => continue,
                Err(_) => {
                    uncertain = true;
                    continue;
                }
            };
            let stat = match self.process(pid) {
                Ok(stat) => stat,
                Err(error) if process_gone(&error) => continue,
                Err(_) => {
                    uncertain = true;
                    continue;
                }
            };
            if stat.group != pgid {
                continue;
            }
            if self.pidfd_process(&fd).ok() != Some(pid)
                || require_running_pidfd(&fd).is_err()
                || self.pidfd_process(&leader).ok() != Some(pgid)
                || require_running_pidfd(&leader).is_err()
            {
                uncertain = true;
                continue;
            }
            match pidfd_send_signal(&fd, signal) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(_) => uncertain = true,
            }
        }
        if uncertain {
            Err(io::Error::other("some group members could not be verified"))
        } else {
            Ok(())
        }
    }

    fn namespace_inode(&self, path: &str) -> io::Result<u64> {
        let inode = statat(&self.root, path, AtFlags::empty())?.st_ino;
        if inode == 0 {
            return Err(invalid("namespace inode is zero"));
        }
        Ok(inode)
    }

    fn read(&self, path: &str) -> io::Result<String> {
        String::from_utf8(self.read_bytes(path)?)
            .map_err(|_| invalid("process evidence is not UTF-8"))
    }

    fn read_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        let fd = openat(
            &self.root,
            path,
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let mut bytes = Vec::new();
        File::from(fd)
            .take(MAX_PROC_READ + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PROC_READ {
            return Err(invalid("process evidence exceeds size limit"));
        }
        Ok(bytes)
    }
}

/// A positive fdinfo PID is not sufficient on older kernels: Linux 5.4 can
/// keep reporting the old number after its task has been reaped. A pidfd that
/// reports exit is not used as a group anchor, even if it is still a zombie.
pub(super) fn require_running_pidfd(fd: impl AsFd) -> io::Result<()> {
    let mut fds = [PollFd::from_borrowed_fd(fd.as_fd(), PollFlags::IN)];
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if poll(&mut fds, Some(&timeout))? != 0 {
        return Err(invalid("pidfd reports the process has exited"));
    }
    Ok(())
}

fn confirm_unreaped_child(pid: Pid) -> io::Result<()> {
    waitid(
        WaitId::Pid(pid),
        WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
    )?;
    Ok(())
}

fn classify_pidfd(fd: impl AsFd) -> io::Result<IdentityStrength> {
    let magic = fstatfs(&fd)?.f_type as u64;
    match magic {
        PID_FS_MAGIC if cfg!(target_pointer_width = "64") => {
            let inode = fstat(fd)?.st_ino;
            if inode == 0 {
                return Err(invalid("pidfs identity is zero"));
            }
            Ok(IdentityStrength::Pidfs { inode })
        }
        ANON_INODE_FS_MAGIC => Ok(IdentityStrength::Approximate {
            reason: UnavailableReason::LegacyPidfd,
        }),
        _ => Err(invalid("unrecognized pidfd identity")),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_local_view(status: &str, self_pid: u32) -> io::Result<()> {
    let mut fields = status
        .lines()
        .filter_map(|line| line.strip_prefix("NSpid:"));
    let Some(value) = fields.next() else {
        return Err(invalid("procfs pid namespace is not verifiable"));
    };
    if fields.next().is_some() {
        return Err(invalid("ambiguous procfs pid namespace"));
    }
    let mut levels = value.split_whitespace();
    if levels.next().and_then(|pid| pid.parse::<u32>().ok()) != Some(self_pid)
        || self_pid == 0
        || levels.next().is_some()
    {
        return Err(invalid("procfs pid namespace does not match caller"));
    }
    Ok(())
}

fn check_sigchld_not_ignored(status: &str) -> io::Result<()> {
    let mut fields = status
        .lines()
        .filter_map(|line| line.strip_prefix("SigIgn:"));
    let ignored = fields
        .next()
        .and_then(|value| u128::from_str_radix(value.trim(), 16).ok())
        .ok_or_else(|| invalid("SIGCHLD disposition is not readable"))?;
    if fields.next().is_some() {
        return Err(invalid("ambiguous SIGCHLD disposition"));
    }
    let child_bit = 1_u128 << (nix::libc::SIGCHLD - 1);
    if ignored & child_bit != 0 {
        return Err(invalid("SIGCHLD is ignored"));
    }
    Ok(())
}

fn parse_boot_id(text: &str) -> io::Result<String> {
    let id = text.trim();
    if id.len() != 36
        || !id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return Err(invalid("invalid boot id"));
    }
    Ok(id.to_ascii_lowercase())
}

/// A process that exits between opening and reading its row reports ESRCH.
fn process_gone(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
        || error.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error())
}

/// The name between the parentheses is raw bytes: the kernel keeps 15 of them,
/// which can end inside a UTF-8 character. Only the fields around it are read.
fn parse_process_stat(bytes: &[u8]) -> io::Result<ProcessStat> {
    let open = bytes
        .iter()
        .position(|byte| *byte == b'(')
        .ok_or_else(|| invalid("invalid process stat"))?;
    let close = bytes
        .iter()
        .rposition(|byte| *byte == b')')
        .filter(|close| *close > open)
        .ok_or_else(|| invalid("invalid process stat name"))?;
    let pid = std::str::from_utf8(&bytes[..open])
        .ok()
        .and_then(|pid| pid.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 0)
        .ok_or_else(|| invalid("invalid process stat pid"))?;
    let rest = std::str::from_utf8(&bytes[close + 1..])
        .map_err(|_| invalid("invalid process stat fields"))?;
    let fields = rest.split_whitespace().collect::<Vec<_>>();
    let state = fields
        .first()
        .and_then(|state| {
            let mut chars = state.chars();
            let first = chars.next()?;
            (first.is_ascii_alphabetic() && chars.next().is_none()).then_some(first)
        })
        .ok_or_else(|| invalid("invalid process stat state"))?;
    let group = fields
        .get(2)
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|group| *group > 0)
        .ok_or_else(|| invalid("invalid process stat group"))?;
    let start_ticks = fields
        .get(19)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| invalid("invalid process stat start"))?;
    Ok(ProcessStat {
        pid,
        group,
        start_ticks,
        state,
    })
}

fn parse_pidfd_pid(text: &str) -> io::Result<i32> {
    let mut fields = text.lines().filter_map(|line| line.strip_prefix("Pid:"));
    let pid = fields
        .next()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .ok_or_else(|| invalid("pidfd process is not verifiable"))?;
    if fields.next().is_some() {
        return Err(invalid("ambiguous pidfd process"));
    }
    // Zero denotes a process invisible to this procfs, and -1 a reaped one.
    // Neither is evidence that a given numeric PID is still bound to this fd.
    if pid <= 0 {
        return Err(invalid("pidfd process is not attached in this view"));
    }
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::process::{Pid, PidfdFlags, pidfd_open};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    #[test]
    fn local_view_requires_one_exact_positive_namespace_level() {
        assert!(check_local_view("Name:\ttest\nNSpid:\t12\n", 12).is_ok());
        for status in [
            "Name:\ttest\n",
            "NSpid:\t12 12\n",
            "NSpid:\t12 13\n",
            "NSpid:\t0\n",
            "NSpid:\t13\n",
            "NSpid:\t12\nNSpid:\t12\n",
        ] {
            assert!(check_local_view(status, 12).is_err(), "{status:?}");
        }
    }

    #[test]
    fn ignored_child_disposition_is_an_unusable_capture_tripwire() {
        assert!(check_sigchld_not_ignored("SigIgn:\t0000000000000000\n").is_ok());
        let bit = 1_u128 << (nix::libc::SIGCHLD - 1);
        assert!(check_sigchld_not_ignored(&format!("SigIgn:\t{bit:x}\n")).is_err());
        assert!(check_sigchld_not_ignored("SigIgn:\tgarbage\n").is_err());
        assert!(check_sigchld_not_ignored("Name:\tfixture\n").is_err());
        assert!(check_sigchld_not_ignored("SigIgn:\t0\nSigIgn:\t0\n").is_err());
    }

    #[test]
    fn parsers_reject_ambiguous_rows_and_accept_names_with_parentheses() {
        let stat = parse_process_stat(
            b"55 (a ) tricky (name)) S 10 55 55 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1234 0",
        )
        .expect("valid stat");
        assert_eq!((stat.pid, stat.group, stat.start_ticks), (55, 55, 1234));
        assert!(parse_process_stat(b"55 (bad) S 1 55").is_err());
        let cut = b"55 (run_\xe3\x83\x86\xe3\x82) S 10 55 55 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1234 0";
        let stat = parse_process_stat(cut).expect("a name cut inside a character");
        assert_eq!((stat.pid, stat.group, stat.start_ticks), (55, 55, 1234));
        assert_eq!(
            parse_pidfd_pid("pos:\t0\nPid:\t55\n").expect("pidfd pid"),
            55
        );
        for text in ["Pid:\t0\n", "Pid:\t-1\n", "Pid:\t55\nPid:\t55\n", ""] {
            assert!(parse_pidfd_pid(text).is_err());
        }
        assert!(parse_boot_id("not a boot id").is_err());
    }

    #[test]
    fn a_name_cut_inside_a_character_does_not_block_member_signals() {
        // The kernel keeps 15 bytes of an executable's name, which can end inside a
        // UTF-8 character. Such a process elsewhere on the host does not make an
        // unrelated group unverifiable.
        struct Reap(std::process::Child);
        impl Drop for Reap {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let view = ProcView::open().expect("local procfs");
        let directory = tempfile::tempdir().expect("tempdir");
        let program = directory.path().join("run_テストサーバー");
        std::fs::copy("/bin/sleep", &program).expect("copy sleep");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        // A test forking in another thread can hold the new file open for writing
        // until its child execs.
        let other = loop {
            match Command::new(&program).arg("60").spawn() {
                Err(error)
                    if error.kind() == io::ErrorKind::ExecutableFileBusy
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                spawned => break Reap(spawned.expect("cut name")),
            }
        };
        let other_pid = other.0.id() as i32;
        while !std::fs::read(format!("/proc/{other_pid}/comm"))
            .is_ok_and(|name| name.starts_with(b"run_"))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "renamed process did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(std::fs::read_to_string(format!("/proc/{other_pid}/comm")).is_err());
        let leader = Reap(
            Command::new("/bin/sleep")
                .arg("60")
                .process_group(0)
                .spawn()
                .expect("fixture leader"),
        );
        let pgid = leader.0.id() as i32;
        let leader_fd = pidfd_open(
            Pid::from_raw(pgid).expect("leader pid"),
            PidfdFlags::empty(),
        )
        .expect("pin fixture leader");
        view.signal_members_in(pgid, &leader_fd, Signal::CONT, vec![other_pid, pgid])
            .expect("an unrelated cut name leaves the group verifiable");
    }

    #[test]
    fn a_process_that_ends_while_its_row_is_read_is_gone() {
        assert!(process_gone(&io::Error::from_raw_os_error(
            rustix::io::Errno::SRCH.raw_os_error()
        )));
        assert!(process_gone(&io::Error::from(io::ErrorKind::NotFound)));
        assert!(!process_gone(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
    }

    #[test]
    fn proc_view_rejects_a_regular_directory() {
        let directory = tempfile::tempdir().expect("tempdir");
        assert!(ProcView::open_path(directory.path()).is_err());
    }

    #[test]
    fn local_proc_and_owned_child_can_be_read_through_one_mount() {
        let proc = ProcView::open().expect("validated local procfs");
        let origin = proc.origin().expect("local origin");
        assert!(!origin.boot_id.is_empty());
        let own = proc.process(std::process::id() as i32).expect("own stat");
        assert_eq!(own.pid, std::process::id() as i32);

        let mut child = Command::new("/bin/sleep")
            .arg("0.1")
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture child");
        let pid = child.id() as i32;
        let stat = proc.process(pid).expect("child stat");
        assert_eq!(stat.pid, pid);
        let captured = proc.capture_child(pid).expect("owned child capture");
        assert_eq!(captured.origin, origin);
        assert_eq!(captured.start_ticks, stat.start_ticks);
        if let Ok(fd) = pidfd_open(
            Pid::from_raw(pid).expect("fixture pid"),
            PidfdFlags::empty(),
        ) {
            assert_eq!(proc.pidfd_process(&fd).expect("child pidfd"), pid);
        }
        child.wait().expect("reap fixture child");
        assert!(
            proc.capture_child(pid).is_err(),
            "an already reaped child cannot become owned evidence"
        );
    }

    #[test]
    fn exited_leader_is_not_authorized_even_when_fdinfo_still_names_it() {
        let view = ProcView::open().expect("local procfs");
        let mut child = Command::new("/bin/cat")
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture child");
        let pid = child.id() as i32;
        let fd = match pidfd_open(
            Pid::from_raw(pid).expect("fixture pid"),
            PidfdFlags::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => {
                drop(child.stdin.take());
                child.wait().expect("fixture exits without a signal");
                return;
            }
        };
        let identity = view.capture_child(pid).expect("capture running child");
        let group = GroupRecord {
            id: uuid::Uuid::new_v4(),
            source_id: None,
            pgid: pid,
            spawned_at: 1,
            observed_at: 1,
            origin: Some(identity.origin.clone()),
            evidence: GroupEvidence::Captured(identity),
        };
        assert!(matches!(
            view.verified_leader(&group),
            Ok(Leader::Pinned(_))
        ));
        assert!(require_running_pidfd(&fd).is_ok());

        drop(child.stdin.take());
        waitid(
            WaitId::Pid(Pid::from_raw(pid).expect("fixture pid")),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        )
        .expect("observe exit without reaping");
        assert_eq!(view.pidfd_process(&fd).expect("unreaped fdinfo"), pid);
        assert!(require_running_pidfd(&fd).is_err());
        assert!(view.verified_leader(&group).is_err());
        child.wait().expect("reap fixture child");
        assert!(require_running_pidfd(&fd).is_err());
    }

    #[test]
    fn exited_leader_cannot_authorize_signalling_a_remaining_member() {
        let view = ProcView::open().expect("local procfs");
        let temp = tempfile::tempdir().expect("fixture directory");
        let marker = temp.path().join("member.pid");
        let mut leader = Command::new("/bin/sh")
            .args([
                "-c",
                "sleep 5 & printf '%s\\n' \"$!\" > \"$1\"; sleep 1",
                "fixture",
            ])
            .arg(&marker)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture group leader");
        let pgid = leader.id() as i32;
        let leader_fd = match pidfd_open(
            Pid::from_raw(pgid).expect("fixture pid"),
            PidfdFlags::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => {
                leader.wait().expect("fixture exits without a signal");
                return;
            }
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(800);
        let member = loop {
            if let Ok(value) = std::fs::read_to_string(&marker)
                && let Ok(pid) = value.trim().parse::<i32>()
            {
                break pid;
            }
            assert!(std::time::Instant::now() < deadline, "member did not start");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let member_fd = pidfd_open(
            Pid::from_raw(member).expect("fixture member pid"),
            PidfdFlags::empty(),
        )
        .expect("pin fixture member");
        assert_eq!(view.parent_pid(member).expect("member parent"), pgid);
        assert_eq!(view.process(member).expect("member stat").group, pgid);
        assert_eq!(
            view.pidfd_process(&member_fd).expect("member fdinfo"),
            member
        );
        assert!(require_running_pidfd(&member_fd).is_ok());
        struct MemberCleanup(std::os::fd::OwnedFd);
        impl Drop for MemberCleanup {
            fn drop(&mut self) {
                let _ = pidfd_send_signal(&self.0, Signal::KILL);
            }
        }
        let member_fd = MemberCleanup(member_fd);

        waitid(
            WaitId::Pid(Pid::from_raw(pgid).expect("fixture leader pid")),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        )
        .expect("observe leader exit without reaping");
        assert_eq!(view.pidfd_process(&leader_fd).expect("leader fdinfo"), pgid);
        assert!(
            view.signal_members_in(pgid, &leader_fd, Signal::TERM, vec![member, pgid])
                .is_err()
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
        while std::time::Instant::now() < deadline {
            assert!(
                require_running_pidfd(&member_fd.0).is_ok(),
                "an exited leader must not authorize a member signal"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        leader.wait().expect("reap fixture leader");
    }

    #[test]
    fn an_exited_member_is_not_treated_as_a_verified_target() {
        let view = ProcView::open().expect("local procfs");
        let mut leader = Command::new("/bin/cat")
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture leader");
        let pgid = leader.id() as i32;
        let leader_fd = match pidfd_open(
            Pid::from_raw(pgid).expect("fixture leader pid"),
            PidfdFlags::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => {
                drop(leader.stdin.take());
                leader.wait().expect("fixture exits without a signal");
                return;
            }
        };
        let mut member = Command::new("/bin/true")
            .process_group(pgid)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture member");
        let member_pid = member.id() as i32;
        let member_fd = pidfd_open(
            Pid::from_raw(member_pid).expect("fixture member pid"),
            PidfdFlags::empty(),
        )
        .expect("pin fixture member");
        waitid(
            WaitId::Pid(Pid::from_raw(member_pid).expect("fixture member pid")),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        )
        .expect("observe member exit without reaping");
        assert_eq!(view.process(member_pid).expect("member stat").group, pgid);
        assert_eq!(
            view.pidfd_process(&member_fd).expect("member fdinfo"),
            member_pid
        );
        assert!(require_running_pidfd(&member_fd).is_err());
        assert!(require_running_pidfd(&leader_fd).is_ok());
        assert!(
            view.signal_members_in(pgid, &leader_fd, Signal::CONT, vec![member_pid, pgid])
                .is_err()
        );
        member.wait().expect("reap fixture member");
        drop(leader.stdin.take());
        leader.wait().expect("reap fixture leader");
    }

    #[test]
    fn recovery_checks_strong_evidence_before_signalling_an_owned_fixture() {
        struct Cleanup(std::process::Child);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if matches!(self.0.try_wait(), Ok(None)) {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }

        let view = ProcView::open().expect("local procfs");
        let mut child = Cleanup(
            Command::new("/bin/sleep")
                .arg("60")
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("fixture child"),
        );
        let captured = view
            .capture_child(child.0.id() as i32)
            .expect("capture child");
        let group = GroupRecord {
            id: uuid::Uuid::new_v4(),
            source_id: None,
            pgid: child.0.id() as i32,
            spawned_at: 1,
            observed_at: 1,
            origin: Some(captured.origin.clone()),
            evidence: GroupEvidence::Captured(captured),
        };

        let mut wrong_start = group.clone();
        if let GroupEvidence::Captured(identity) = &mut wrong_start.evidence {
            identity.start_ticks += 1;
        }
        assert!(view.signal_group(&wrong_start, Signal::TERM).is_err());
        assert!(child.0.try_wait().expect("poll fixture").is_none());

        if let GroupEvidence::Captured(CapturedIdentity {
            strength: IdentityStrength::Pidfs { inode },
            ..
        }) = &group.evidence
        {
            let mut wrong_inode = group.clone();
            if let GroupEvidence::Captured(identity) = &mut wrong_inode.evidence {
                identity.strength = IdentityStrength::Pidfs { inode: inode + 1 };
            }
            assert!(view.signal_group(&wrong_inode, Signal::TERM).is_err());
            assert!(child.0.try_wait().expect("poll fixture").is_none());
        }

        let result = view.signal_group(&group, Signal::TERM);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while child.0.try_wait().expect("poll fixture").is_none()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            child.0.try_wait().expect("poll fixture").is_some(),
            "verified fixture was not signalled: {result:?}"
        );
    }
}
