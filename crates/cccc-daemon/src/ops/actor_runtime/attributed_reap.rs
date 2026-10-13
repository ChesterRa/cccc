//! Bounded retirement cleanup for processes that outlived their Group's sessions.
//!
//! A session stop kills the process tree the runtime owns (see
//! `crates/cccc-runtime/src/process_tree`). A child that detached into its own
//! session — `setsid`, or a provider CLI that daemonises itself — is no longer
//! part of that tree and survives the stop, so retiring a Group (delete, or
//! Reset retiring the old Group) can leave live processes behind.
//!
//! Every Actor process is launched with `CCCC_HOME`, `CCCC_GROUP_ID` and
//! `CCCC_ACTOR_ID`, which keeps those survivors attributable after they leave
//! the tree. A process is attributed to the retired Group only when all of
//! these hold:
//! - it runs under the daemon's real UID;
//! - its `CCCC_GROUP_ID` equals the Group ID;
//! - its `CCCC_HOME`, canonicalised, equals the daemon's effective home.
//!   Group IDs are only unique within one home, so another home's Group with
//!   the same ID is never touched.
//!
//! A process is identified by its PID together with its kernel start time
//! (`/proc/<pid>/stat` field 22), and the identity plus attribution is read
//! again before SIGTERM and again before SIGKILL; anything that no longer
//! matches is left alone. Signals still use numeric PIDs, so a process that
//! exits after revalidation can have its PID reused before `kill(2)`. These
//! checks narrow that window; they do not give a pidfd's identity guarantee.
//!
//! This is a one-shot sweep at retirement, not a background reaper.

use cccc_core::HomeLayout;

/// What became of one attributed process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub enum ReapOutcome {
    /// Confirmed gone: the PID no longer exists, is a zombie awaiting its
    /// parent, or names a process with a different start time.
    Exited,
    /// Still the recorded, attributed process after the final wait, or a
    /// signal to it could not be delivered.
    Running,
    /// Could not be revalidated, so it was not signalled (again): before
    /// SIGTERM the PID named a different process start, or the recorded start
    /// no longer carries the recorded attribution or its environment can no
    /// longer be read.
    IdentityChanged,
}

/// One process the retirement sweep found attributed to the Group.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReapedProcess {
    pub pid: u32,
    /// Kernel start time in clock ticks since boot; with `pid`, the identity
    /// that was revalidated before each signal.
    pub start_time: u64,
    pub actor_id: String,
    /// SIGTERM was delivered (the kernel accepted it), not that it worked.
    pub signaled_term: bool,
    /// SIGKILL was delivered after the TERM grace expired.
    pub signaled_kill: bool,
    pub outcome: ReapOutcome,
}

/// Stops every process attributed to `group_id` in `home` (see the module docs)
/// and reports each one.
///
/// Call it only once the Group's sessions have been stopped, so it only sees
/// what the process-tree kill could not reach. Off Linux there is no readable
/// process environment, and the result is always empty.
pub fn reap_group(home: &HomeLayout, group_id: &str) -> Vec<ReapedProcess> {
    #[cfg(target_os = "linux")]
    {
        linux::reap(home.root(), group_id, linux::TERM_GRACE, linux::KILL_GRACE)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (home, group_id);
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ReapOutcome, ReapedProcess};
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    pub(super) const TERM_GRACE: Duration = Duration::from_millis(1500);
    pub(super) const KILL_GRACE: Duration = Duration::from_millis(1000);
    const POLL: Duration = Duration::from_millis(50);

    /// The attribution a process must carry to belong to the retired Group.
    pub(super) struct Target {
        group_id: Vec<u8>,
        home: PathBuf,
        uid: u32,
    }

    impl Target {
        pub(super) fn new(home: &Path, group_id: &str) -> Option<Self> {
            // An empty CCCC_GROUP_ID marks Group-less (global) processes.
            if group_id.is_empty() {
                return None;
            }
            Some(Self {
                group_id: group_id.as_bytes().to_vec(),
                home: canonical(home),
                uid: real_uid("self")?,
            })
        }
    }

    /// A process recorded by the scan: PID, start time and attribution.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct Identity {
        pub(super) pid: u32,
        pub(super) start_time: u64,
        pub(super) actor_id: String,
    }

    /// The parts of `/proc/<pid>/stat` the sweep relies on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Stat {
        pub(super) state: u8,
        pub(super) start_time: u64,
    }

    impl Stat {
        /// Zombie or dead: the process has terminated.
        fn terminated(self) -> bool {
            matches!(self.state, b'Z' | b'X' | b'x')
        }
    }

    /// A recorded identity compared with what the PID names now.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum Check {
        /// Same start time and same attribution.
        Same,
        /// Same start time, but its environment cannot be read right now.
        /// A process that is exiting looks like this (its memory is torn down
        /// before it becomes a zombie), and so does one that turned
        /// non-dumpable. It cannot be revalidated, so it is waited on but never
        /// signalled.
        Unreadable,
        /// No such PID, or it has terminated.
        Gone,
        /// The PID names a different process start.
        Replaced,
        /// Same process start, but it no longer carries the recorded
        /// attribution.
        Released,
    }

    /// What a process's environment says about its attribution.
    enum Attribution {
        Ours(String),
        Foreign,
        Unreadable,
    }

    pub(super) fn reap(
        home: &Path,
        group_id: &str,
        term_grace: Duration,
        kill_grace: Duration,
    ) -> Vec<ReapedProcess> {
        let Some(target) = Target::new(home, group_id) else {
            return Vec::new();
        };
        retire(scan(&target), &target, term_grace, kill_grace)
    }

    /// Every live process in `/proc` attributed to `target`, except ourselves.
    pub(super) fn scan(target: &Target) -> Vec<Identity> {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let own = std::process::id();
        let mut found = entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| *pid != own)
            .filter_map(|pid| observe(pid, target))
            .collect::<Vec<_>>();
        found.sort_by_key(|identity| identity.pid);
        found
    }

    /// Signals the recorded processes: SIGTERM, a grace, then SIGKILL for the
    /// ones still running, revalidating the identity before each signal.
    pub(super) fn retire(
        recorded: Vec<Identity>,
        target: &Target,
        term_grace: Duration,
        kill_grace: Duration,
    ) -> Vec<ReapedProcess> {
        let mut entries = recorded
            .into_iter()
            .map(|identity| {
                let mut report = ReapedProcess {
                    pid: identity.pid,
                    start_time: identity.start_time,
                    actor_id: identity.actor_id.clone(),
                    signaled_term: false,
                    signaled_kill: false,
                    outcome: ReapOutcome::Running,
                };
                let pending = match check(&identity, target) {
                    Check::Same => deliver(&identity, Signal::SIGTERM, &mut report),
                    Check::Unreadable => true,
                    Check::Gone => settle(&mut report, ReapOutcome::Exited),
                    // Never signalled: whatever the PID names now is not ours.
                    Check::Replaced | Check::Released => {
                        settle(&mut report, ReapOutcome::IdentityChanged)
                    }
                };
                (identity, report, pending)
            })
            .collect::<Vec<_>>();

        wait_for_exit(&mut entries, target, term_grace);
        for (identity, report, pending) in &mut entries {
            if !*pending {
                continue;
            }
            *pending = match check(identity, target) {
                Check::Same => deliver(identity, Signal::SIGKILL, report),
                other => still_pending(report, other),
            };
        }
        wait_for_exit(&mut entries, target, kill_grace);
        for (identity, report, pending) in &mut entries {
            if !*pending {
                continue;
            }
            // Still there after the final wait. One that can no longer be
            // revalidated is not reported as ours; a revalidated one keeps
            // `outcome: Running`.
            match check(identity, target) {
                Check::Same => {}
                Check::Gone | Check::Replaced => report.outcome = ReapOutcome::Exited,
                Check::Unreadable | Check::Released => {
                    report.outcome = ReapOutcome::IdentityChanged;
                }
            }
        }
        entries.into_iter().map(|(_, report, _)| report).collect()
    }

    /// Sends `signal_kind` to a revalidated process and records the delivery;
    /// true while the process stays pending.
    fn deliver(identity: &Identity, signal_kind: Signal, report: &mut ReapedProcess) -> bool {
        match signal(identity.pid, signal_kind) {
            Ok(()) => {
                if signal_kind == Signal::SIGKILL {
                    report.signaled_kill = true;
                } else {
                    report.signaled_term = true;
                }
                true
            }
            Err(Errno::ESRCH) => settle(report, ReapOutcome::Exited),
            Err(_) => settle(report, ReapOutcome::Running),
        }
    }

    /// Records a final outcome and reports the entry as no longer pending.
    fn settle(report: &mut ReapedProcess, outcome: ReapOutcome) -> bool {
        report.outcome = outcome;
        false
    }

    /// While waiting on a process that was signalled (or could not be read),
    /// a PID naming a different process start means ours is gone. A process
    /// that is exiting can briefly read as unreadable or as carrying a partial
    /// environment, so neither ends the wait; they only withhold the next
    /// signal, which goes out solely on `Check::Same`.
    fn still_pending(report: &mut ReapedProcess, check: Check) -> bool {
        match check {
            Check::Same | Check::Unreadable | Check::Released => true,
            Check::Gone | Check::Replaced => settle(report, ReapOutcome::Exited),
        }
    }

    fn wait_for_exit(
        entries: &mut [(Identity, ReapedProcess, bool)],
        target: &Target,
        grace: Duration,
    ) {
        let deadline = Instant::now() + grace;
        loop {
            for (identity, report, pending) in entries.iter_mut() {
                if *pending {
                    *pending = still_pending(report, check(identity, target));
                }
            }
            if entries.iter().all(|(_, _, pending)| !pending) || Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(POLL);
        }
    }

    pub(super) fn check(identity: &Identity, target: &Target) -> Check {
        let (start_time, attribution) = match read_consistently(identity.pid, target) {
            Ok(read) => read,
            Err(check) => return check,
        };
        if start_time != identity.start_time {
            return Check::Replaced;
        }
        match attribution {
            Attribution::Ours(actor_id) if actor_id == identity.actor_id => Check::Same,
            Attribution::Unreadable => Check::Unreadable,
            Attribution::Ours(_) | Attribution::Foreign => Check::Released,
        }
    }

    /// The live, attributed identity behind `pid`, if any.
    pub(super) fn observe(pid: u32, target: &Target) -> Option<Identity> {
        match read_consistently(pid, target) {
            Ok((start_time, Attribution::Ours(actor_id))) => Some(Identity {
                pid,
                start_time,
                actor_id,
            }),
            _ => None,
        }
    }

    /// Reads the start time and attribution of a live `pid`. The start time is
    /// read on both sides of the environment read, so the attribution belongs
    /// to that process start and not to a PID reused in between; a change in
    /// between reports `Replaced`, termination `Gone`.
    fn read_consistently(pid: u32, target: &Target) -> Result<(u64, Attribution), Check> {
        let live = || match read_stat(pid) {
            Some(stat) if !stat.terminated() => Ok(stat.start_time),
            _ => Err(Check::Gone),
        };
        let before = live()?;
        let attribution = attribution(pid, target);
        if live()? != before {
            return Err(Check::Replaced);
        }
        Ok((before, attribution))
    }

    /// Whether a process carries `target`'s attribution, and its `CCCC_ACTOR_ID`.
    ///
    /// An environment we cannot read (another user's process, a non-dumpable
    /// one, or one whose memory is already torn down on exit, which reads as
    /// empty, `ESRCH` or `EACCES`) is never attributed: never signal what we
    /// cannot read.
    fn attribution(pid: u32, target: &Target) -> Attribution {
        let environ = match std::fs::read(format!("/proc/{pid}/environ")) {
            Ok(environ) if !environ.is_empty() => environ,
            _ => return Attribution::Unreadable,
        };
        let matches = || {
            if env_value(&environ, b"CCCC_GROUP_ID")? != target.group_id.as_slice() {
                return None;
            }
            let home = Path::new(OsStr::from_bytes(env_value(&environ, b"CCCC_HOME")?));
            if !home.is_absolute() || canonical(home) != target.home {
                return None;
            }
            if real_uid(&pid.to_string())? != target.uid {
                return None;
            }
            let actor_id = env_value(&environ, b"CCCC_ACTOR_ID").unwrap_or_default();
            Some(String::from_utf8_lossy(actor_id).into_owned())
        };
        matches().map_or(Attribution::Foreign, Attribution::Ours)
    }

    /// The first `key=value` entry of a NUL-separated environment, as getenv
    /// would see it.
    pub(super) fn env_value<'a>(environ: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
        environ.split(|byte| *byte == 0).find_map(|entry| {
            entry
                .strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(b"="))
        })
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn read_stat(pid: u32) -> Option<Stat> {
        parse_stat(&std::fs::read(format!("/proc/{pid}/stat")).ok()?)
    }

    /// Parses `/proc/<pid>/stat`. The command name (field 2) sits in
    /// parentheses and may itself contain spaces, `)` or ` Z `, so the fields
    /// are only read after the last `)`: state (field 3) first, start time
    /// (field 22) nineteen fields later.
    pub(super) fn parse_stat(raw: &[u8]) -> Option<Stat> {
        let close = raw.iter().rposition(|byte| *byte == b')')?;
        let rest = std::str::from_utf8(&raw[close + 1..]).ok()?;
        let mut fields = rest.split_ascii_whitespace();
        let state = fields.next().filter(|state| state.len() == 1)?.as_bytes()[0];
        let start_time = fields.nth(18)?.parse().ok()?;
        Some(Stat { state, start_time })
    }

    /// The real UID from `/proc/<pid>/status`.
    fn real_uid(pid: &str) -> Option<u32> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    }

    fn signal(pid: u32, signal: Signal) -> Result<(), Errno> {
        let pid = i32::try_from(pid).map_err(|_| Errno::ESRCH)?;
        kill(Pid::from_raw(pid), signal)
    }

    #[cfg(test)]
    pub(crate) mod fixture {
        //! Attributed child processes for tests, killed and waited on drop.

        use std::io::{BufRead, BufReader};
        use std::path::Path;
        use std::process::{Child, Command, Stdio};

        pub(crate) struct Fixture {
            child: Child,
        }

        impl Fixture {
            /// Runs `program args...` with the given attribution. Stdin is a
            /// pipe held open by the fixture, so `read` blocks without
            /// spawning helpers. The shell reports ready after its signal
            /// handling is installed and any `setsid` exec has completed.
            pub(crate) fn spawn(
                program: &Path,
                args: &[&str],
                home: &Path,
                group_id: &str,
                actor_id: &str,
            ) -> Self {
                let child = Command::new(program)
                    .args(args)
                    .env("CCCC_HOME", home)
                    .env("CCCC_GROUP_ID", group_id)
                    .env("CCCC_ACTOR_ID", actor_id)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn fixture child");
                let mut fixture = Self { child };
                fixture.wait_ready();
                fixture
            }

            /// A shell blocked in `read`; `trap_term` makes it ignore SIGTERM.
            pub(crate) fn shell(home: &Path, group_id: &str, trap_term: bool) -> Self {
                Self::spawn(
                    Path::new("/bin/sh"),
                    &["-c", script(trap_term)],
                    home,
                    group_id,
                    "fixture",
                )
            }

            /// The same shell in a new session (`setsid` execs in place since
            /// the child is not a process-group leader): the shape a
            /// process-tree kill cannot reach.
            pub(crate) fn detached(home: &Path, group_id: &str) -> Self {
                Self::spawn(
                    Path::new("setsid"),
                    &["/bin/sh", "-c", script(false)],
                    home,
                    group_id,
                    "detached",
                )
            }

            pub(crate) fn pid(&self) -> u32 {
                self.child.id()
            }

            /// Running and not a zombie.
            pub(crate) fn alive(&self) -> bool {
                super::read_stat(self.pid()).is_some_and(|stat| !stat.terminated())
            }

            fn wait_ready(&mut self) {
                let stdout = self.child.stdout.take().expect("fixture stdout pipe");
                let mut ready = String::new();
                BufReader::new(stdout)
                    .read_line(&mut ready)
                    .expect("read fixture ready handshake");
                assert_eq!(ready, "cccc-reap-ready\n", "shell fixture became ready");
            }
        }

        pub(super) fn script(trap_term: bool) -> &'static str {
            if trap_term {
                "trap '' TERM; printf 'cccc-reap-ready\\n'; read -r _"
            } else {
                "printf 'cccc-reap-ready\\n'; read -r _"
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::fixture::Fixture;
        use super::*;
        use std::sync::atomic::{AtomicU32, Ordering};

        const TEST_TERM_GRACE: Duration = Duration::from_millis(400);
        const TEST_KILL_GRACE: Duration = Duration::from_secs(3);

        fn unique_group() -> String {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            format!(
                "g_reap_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )
        }

        fn home() -> tempfile::TempDir {
            tempfile::tempdir().expect("home")
        }

        fn reap_now(home: &Path, group_id: &str) -> Vec<ReapedProcess> {
            reap(home, group_id, TEST_TERM_GRACE, TEST_KILL_GRACE)
        }

        fn entry(reaped: &[ReapedProcess], pid: u32) -> &ReapedProcess {
            reaped
                .iter()
                .find(|entry| entry.pid == pid)
                .unwrap_or_else(|| panic!("{pid} not in {reaped:?}"))
        }

        fn stat_line(comm: &str, state: char, start_time: u64) -> Vec<u8> {
            // Fields 4..21 are filler; field 22 is the start time.
            let filler = (4..22).map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
            format!("4242 ({comm}) {state} {filler} {start_time} 23 24\n").into_bytes()
        }

        #[test]
        fn stat_state_and_start_time_come_after_the_last_paren() {
            for comm in ["sleep", "a Z b", "x) Z (y", ") Z )", "a b c d e f g"] {
                let stat = parse_stat(&stat_line(comm, 'S', 987_654)).expect("parses");
                assert_eq!(stat.state, b'S', "comm {comm:?}");
                assert_eq!(stat.start_time, 987_654, "comm {comm:?}");
                assert!(!stat.terminated(), "comm {comm:?}");
            }
            assert!(
                parse_stat(&stat_line("worker", 'Z', 1))
                    .expect("parses")
                    .terminated()
            );
            assert_eq!(parse_stat(b"4242 (truncated) S 1 2"), None);
            assert_eq!(parse_stat(b"no parens at all"), None);
        }

        #[test]
        fn env_value_takes_the_exact_key_and_first_entry() {
            let environ = b"CCCC_GROUP_ID_X=no\0CCCC_GROUP_ID=g1\0CCCC_GROUP_ID=g2\0";
            assert_eq!(env_value(environ, b"CCCC_GROUP_ID"), Some(b"g1".as_slice()));
            assert_eq!(env_value(environ, b"CCCC_HOME"), None);
        }

        #[test]
        fn fixture_ready_confirms_signal_handling_and_detached_session() {
            let home = home();
            let group_id = unique_group();
            let resistant = Fixture::shell(home.path(), &group_id, true);
            let status = std::fs::read_to_string(format!("/proc/{}/status", resistant.pid()))
                .expect("fixture status");
            let ignored = status
                .lines()
                .find_map(|line| line.strip_prefix("SigIgn:"))
                .expect("ignored signal mask");
            let ignored = u64::from_str_radix(ignored.trim(), 16).expect("parse signal mask");
            assert_ne!(ignored & (1 << (Signal::SIGTERM as u32 - 1)), 0);

            let detached = Fixture::detached(home.path(), &group_id);
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", detached.pid()))
                .expect("detached fixture stat");
            let rest = stat.rsplit_once(')').expect("stat command delimiter").1;
            let session = rest
                .split_ascii_whitespace()
                .nth(3)
                .expect("session ID")
                .parse::<u32>()
                .expect("parse session ID");
            assert_eq!(session, detached.pid());
        }

        #[test]
        fn reaps_a_detached_process_of_this_home_and_group() {
            let home = home();
            let group_id = unique_group();
            let child = Fixture::detached(home.path(), &group_id);

            let reaped = reap_now(home.path(), &group_id);

            assert_eq!(reaped.len(), 1, "{reaped:?}");
            let entry = entry(&reaped, child.pid());
            assert_eq!(entry.actor_id, "detached");
            assert!(entry.signaled_term && !entry.signaled_kill, "{entry:?}");
            assert_eq!(entry.outcome, ReapOutcome::Exited);
            assert!(!child.alive());
        }

        #[test]
        fn another_home_with_the_same_group_id_is_left_alone() {
            let (home_a, home_b) = (home(), home());
            let group_id = unique_group();
            let in_a = Fixture::shell(home_a.path(), &group_id, false);
            let in_b = Fixture::shell(home_b.path(), &group_id, false);
            // The same home reached through a symlink is still this home.
            let link = home_b.path().join("link-to-a");
            std::os::unix::fs::symlink(home_a.path(), &link).expect("symlink");
            let via_link = Fixture::shell(&link, &group_id, false);

            let reaped = reap_now(home_a.path(), &group_id);

            let mut pids = reaped.iter().map(|entry| entry.pid).collect::<Vec<_>>();
            pids.sort_unstable();
            let mut expected = vec![in_a.pid(), via_link.pid()];
            expected.sort_unstable();
            assert_eq!(pids, expected, "{reaped:?}");
            assert!(reaped.iter().all(|e| e.outcome == ReapOutcome::Exited));
            assert!(!in_a.alive() && !via_link.alive());
            assert!(in_b.alive(), "home B's process died in home A's sweep");
        }

        #[test]
        fn other_groups_are_left_alone() {
            let home = home();
            let kept = Fixture::shell(home.path(), &unique_group(), false);

            let reaped = reap_now(home.path(), &unique_group());

            assert!(reaped.is_empty(), "{reaped:?}");
            assert!(kept.alive());
        }

        #[test]
        fn a_term_resistant_process_is_killed_and_reported_as_such() {
            let home = home();
            let group_id = unique_group();
            let child = Fixture::shell(home.path(), &group_id, true);

            let reaped = reap_now(home.path(), &group_id);

            let entry = entry(&reaped, child.pid());
            assert!(entry.signaled_term && entry.signaled_kill, "{entry:?}");
            assert_eq!(entry.outcome, ReapOutcome::Exited);
            assert!(!child.alive());
        }

        #[test]
        fn a_live_process_named_with_z_is_not_mistaken_for_a_zombie() {
            let home = home();
            let group_id = unique_group();
            let bin = tempfile::tempdir().expect("bin");
            let program = bin.path().join("a Z )b");
            std::os::unix::fs::symlink("/bin/sh", &program).expect("symlink");
            let child = Fixture::spawn(
                &program,
                &["-c", fixture::script(true)],
                home.path(),
                &group_id,
                "z-named",
            );
            let comm =
                std::fs::read_to_string(format!("/proc/{}/comm", child.pid())).expect("comm");
            assert!(comm.contains(" Z "), "fixture comm is {comm:?}");
            let target = Target::new(home.path(), &group_id).expect("target");
            assert!(observe(child.pid(), &target).is_some());

            let reaped = reap_now(home.path(), &group_id);

            // Alive through the TERM grace, so the sweep had to escalate.
            let entry = entry(&reaped, child.pid());
            assert!(entry.signaled_term && entry.signaled_kill, "{entry:?}");
            assert_eq!(entry.outcome, ReapOutcome::Exited);
            assert!(!child.alive());
        }

        #[test]
        fn a_changed_start_time_is_never_signalled() {
            let home = home();
            let group_id = unique_group();
            let child = Fixture::shell(home.path(), &group_id, false);
            let target = Target::new(home.path(), &group_id).expect("target");
            let mut recorded = scan(&target);
            assert_eq!(recorded.len(), 1, "{recorded:?}");
            // As if the recorded process exited and its PID was reused.
            recorded[0].start_time += 1;

            let reaped = retire(recorded, &target, TEST_TERM_GRACE, TEST_KILL_GRACE);

            let entry = entry(&reaped, child.pid());
            assert!(!entry.signaled_term && !entry.signaled_kill, "{entry:?}");
            assert_eq!(entry.outcome, ReapOutcome::IdentityChanged);
            assert!(child.alive(), "a mismatched identity was signalled");
        }

        #[test]
        fn a_changed_attribution_is_never_signalled() {
            let home = home();
            let group_id = unique_group();
            let child = Fixture::shell(home.path(), &group_id, false);
            let target = Target::new(home.path(), &group_id).expect("target");
            let mut recorded = scan(&target);
            assert_eq!(recorded.len(), 1, "{recorded:?}");
            recorded[0].actor_id = "someone-else".into();

            let reaped = retire(recorded, &target, TEST_TERM_GRACE, TEST_KILL_GRACE);

            let entry = entry(&reaped, child.pid());
            assert!(!entry.signaled_term && !entry.signaled_kill, "{entry:?}");
            assert_eq!(entry.outcome, ReapOutcome::IdentityChanged);
            assert!(child.alive());
        }

        #[test]
        fn an_already_exited_process_is_reported_without_a_signal() {
            let home = home();
            let group_id = unique_group();
            let child = Fixture::shell(home.path(), &group_id, false);
            let target = Target::new(home.path(), &group_id).expect("target");
            let recorded = scan(&target);
            let pid = child.pid();
            drop(child);

            let reaped = retire(recorded, &target, TEST_TERM_GRACE, TEST_KILL_GRACE);

            let entry = entry(&reaped, pid);
            assert!(!entry.signaled_term && !entry.signaled_kill, "{entry:?}");
            assert_eq!(entry.outcome, ReapOutcome::Exited);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod retirement_tests {
    //! Group delete and Reset retire processes through `reap_group`.

    use super::linux::fixture::Fixture;
    use cccc_contracts::DaemonRequest;
    use cccc_core::{GroupStore, HomeLayout};
    use serde_json::{Value, json};

    fn request(op: &str, args: Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().expect("args"),
        }
    }

    fn pids(result: &Value) -> Vec<u64> {
        result
            .as_array()
            .expect("reaped list")
            .iter()
            .map(|entry| entry["pid"].as_u64().expect("pid"))
            .collect()
    }

    #[test]
    fn group_delete_reaps_only_this_homes_escaped_processes() {
        let (temp_a, temp_b) = (
            tempfile::tempdir().expect("a"),
            tempfile::tempdir().expect("b"),
        );
        let home = HomeLayout::from_path(temp_a.path()).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("doomed", "")
            .expect("group");
        let ours = Fixture::detached(home.root(), &group.group_id);
        let other_home = Fixture::detached(temp_b.path(), &group.group_id);

        let response = crate::dispatch::dispatch(
            &home,
            &request(
                "group_delete",
                json!({"group_id":group.group_id,"by":"user"}),
            ),
        );

        assert!(response.ok, "{:?}", response.error);
        let result = response.result;
        assert_eq!(result["deleted"], true);
        assert_eq!(
            pids(&result["reaped_processes"]),
            vec![u64::from(ours.pid())]
        );
        let entry = &result["reaped_processes"][0];
        assert_eq!(entry["signaled_term"], true);
        assert_eq!(entry["signaled_kill"], false);
        assert_eq!(entry["outcome"], "exited");
        assert_eq!(entry["actor_id"], "detached");
        assert!(entry["start_time"].as_u64().is_some());
        assert!(!ours.alive());
        assert!(other_home.alive());
    }

    #[test]
    fn group_reset_reaps_the_retired_groups_escaped_processes() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let old = GroupStore::new(home.clone())
            .expect("store")
            .create("reset me", "")
            .expect("group");
        let escaped = Fixture::detached(home.root(), &old.group_id);

        let response = crate::dispatch::dispatch(
            &home,
            &request(
                "group_reset",
                json!({"group_id":old.group_id,"confirm":old.group_id,"by":"user"}),
            ),
        );

        assert!(response.ok, "{:?}", response.error);
        let result = response.result;
        assert_eq!(result["deleted_old"], true);
        assert_ne!(result["new_group_id"], json!(old.group_id));
        assert_eq!(
            pids(&result["old_reaped_processes"]),
            vec![u64::from(escaped.pid())]
        );
        assert_eq!(result["old_reaped_processes"][0]["outcome"], "exited");
        assert!(!escaped.alive());
    }
}
