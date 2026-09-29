use super::*;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::Child;

const T: u64 = 1_000_000;

fn entry(spawned_at: u64, alive_until: u64) -> LedgerEntry {
    LedgerEntry {
        pgid: 500,
        spawned_at,
        alive_until,
    }
}

fn row(pid: i32, started_at: u64) -> ProcessRow {
    ProcessRow {
        pid,
        pgid: 500,
        started_at,
    }
}

#[test]
fn elapsed_time_parses_every_ps_format() {
    assert_eq!(parse_elapsed("00:07"), Some(7));
    assert_eq!(parse_elapsed("12:34"), Some(754));
    assert_eq!(parse_elapsed("01:02:03"), Some(3_723));
    assert_eq!(parse_elapsed("2-01:02:03"), Some(176_523));
    assert_eq!(parse_elapsed("soon"), None);
}

#[test]
fn a_live_leader_is_the_recorded_group_only_with_its_spawn_time() {
    let recorded = entry(T, T + 600);
    assert_eq!(classify(&recorded, &[row(500, T + 1)]), Verdict::Terminate);
    // A recycled PID leads a new group: the recorded one ended.
    assert_eq!(classify(&recorded, &[row(500, T + 3_600)]), Verdict::Forget);
    assert_eq!(classify(&recorded, &[]), Verdict::Forget);
}

#[test]
fn members_of_a_reaped_leader_are_ours_if_they_started_before_the_owner_can_have_died() {
    // The owner refreshed at T + 580, so it died no later than T + 600.
    let recorded = entry(T, T + 600);
    assert_eq!(
        classify(&recorded, &[row(501, T + 10), row(502, T + 900)]),
        Verdict::Terminate
    );
    // Started between heartbeats, after the last refresh but before any death.
    assert_eq!(
        classify(&recorded, &[row(503, T + 595)]),
        Verdict::Terminate
    );
    // Every survivor started after the owner's latest death: possibly a later
    // group that reused the PID. Keep the record rather than drop or signal it.
    assert_eq!(classify(&recorded, &[row(502, T + 900)]), Verdict::Retain);
}

fn orphan_leader() -> Child {
    Command::new("sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .expect("spawn group leader")
}

/// An orphan's new parent may reap it late; a zombie has still exited.
fn exited_within(pid: i32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !running(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn a_recorded_leader_left_behind_is_terminated() {
    let mut leader = orphan_leader();
    let pgid = leader.id() as i32;
    let retained = reconcile(&[LedgerEntry {
        pgid,
        spawned_at: now(),
        alive_until: now(),
    }]);
    assert!(retained.is_empty());
    leader.wait().expect("reap terminated leader");
}

#[test]
fn a_recycled_pid_with_another_start_time_is_never_signalled() {
    let mut unrelated = orphan_leader();
    let pgid = unrelated.id() as i32;
    let retained = reconcile(&[LedgerEntry {
        pgid,
        spawned_at: now() - 3_600,
        alive_until: now() - 3_000,
    }]);
    assert!(retained.is_empty());
    assert!(
        unrelated.try_wait().expect("poll unrelated").is_none(),
        "an unrelated process must not be signalled"
    );
    unrelated.kill().expect("kill unrelated process");
    unrelated.wait().expect("reap unrelated process");
}

#[test]
fn survivors_of_a_reaped_leader_are_terminated() {
    let mut leader = Command::new("sh")
        .args(["-c", "sleep 60 & echo $!"])
        .process_group(0)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn leader with a background member");
    let spawned_at = now();
    let pgid = leader.id() as i32;
    let mut line = String::new();
    BufReader::new(leader.stdout.take().expect("leader stdout"))
        .read_line(&mut line)
        .expect("member pid");
    let member: i32 = line.trim().parse().expect("numeric member pid");
    // The leader exits and is reaped; only its member keeps the group alive.
    leader.wait().expect("reap leader");

    let retained = reconcile(&[LedgerEntry {
        pgid,
        spawned_at,
        alive_until: now(),
    }]);

    assert!(retained.is_empty());
    assert!(
        exited_within(member, Duration::from_secs(5)),
        "a surviving member of a reaped leader was left running"
    );
}

const STALL_MODE: &str = "CCCC_GUARD_STALL_TEST";
const STALL_DIR: &str = "CCCC_GUARD_STALL_DIR";

#[test]
fn a_stalled_watchdog_never_blocks_publication_or_forced_exit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut helper = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "process_tree::guard::tests::stalled_watchdog_helper",
            "--exact",
            "--nocapture",
        ])
        .env(STALL_MODE, "1")
        .env(STALL_DIR, temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn owner helper");
    let deadline = Instant::now() + Duration::from_secs(20);
    let finished = loop {
        if helper.try_wait().expect("poll helper").is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if !finished {
        let _ = helper.kill();
        let _ = helper.wait();
    }
    // The stopped watchdog never exits by itself.
    if let Ok(pid) = std::fs::read_to_string(temp.path().join("watchdog.pid")) {
        let _ = Command::new("kill").args(["-KILL", pid.trim()]).status();
    }
    assert!(
        finished,
        "publication or forced exit blocked on a stalled watchdog"
    );
    assert!(
        temp.path().join("done").exists(),
        "owner helper did not finish"
    );
}

#[test]
fn stalled_watchdog_helper() {
    if std::env::var_os(STALL_MODE).is_none() {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os(STALL_DIR).expect("stall dir"));
    crate::protect_owned_process_groups(&dir.join("ledger.json")).expect("protect");
    let output = Command::new("pgrep")
        .args(["-P", &std::process::id().to_string(), "-x", "sh"])
        .output()
        .expect("pgrep");
    let watchdog = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    std::fs::write(dir.join("watchdog.pid"), &watchdog).expect("publish watchdog");
    assert!(
        Command::new("kill")
            .args(["-STOP", &watchdog])
            .status()
            .expect("stop watchdog")
            .success()
    );
    // Far more than a pipe buffer; these PGIDs exceed every pid_max. Publication
    // must not depend on the watchdog reading anything.
    let groups = (0..20_000)
        .map(|index| OwnedGroup {
            pgid: 900_000_000 + index,
            spawned_at: now(),
        })
        .collect::<Vec<_>>();
    for _ in 0..20 {
        publish(&groups).expect("publish");
    }
    publish(&[]).expect("publish");
    crate::force_terminate_owned().expect("force terminate");
    std::fs::write(dir.join("done"), b"done").expect("publish done");
}

fn run_watchdog(watch_list: &Path) -> std::process::ExitStatus {
    watchdog_command(watch_list)
        .stdin(Stdio::null())
        .status()
        .expect("run watchdog")
}

#[test]
fn the_watchdog_terminates_groups_in_a_fresh_watch_list() {
    let temp = tempfile::tempdir().expect("tempdir");
    let list = temp.path().join("groups.watchdog");
    let mut leader = orphan_leader();
    write_watch_list(
        &list,
        &[OwnedGroup {
            pgid: leader.id() as i32,
            spawned_at: now(),
        }],
    )
    .expect("watch list");
    assert!(run_watchdog(&list).success());
    // The leader is this test's child: poll its exit rather than signal 0,
    // which a zombie still answers.
    let deadline = Instant::now() + Duration::from_secs(5);
    while leader.try_wait().expect("poll leader").is_none() {
        assert!(
            Instant::now() < deadline,
            "the watchdog left a listed group running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_watchdog_leaves_a_stale_watch_list_to_verified_reconciliation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let list = temp.path().join("groups.watchdog");
    let mut unrelated = orphan_leader();
    let written_long_ago = now() - WATCH_LIST_MAX_AGE.as_secs() - 60;
    std::fs::write(&list, format!("{written_long_ago} {}\n", unrelated.id())).expect("list");
    run_watchdog(&list);
    assert!(
        unrelated.try_wait().expect("poll").is_none(),
        "a stale watch list must not be trusted"
    );
    unrelated.kill().expect("kill unrelated");
    unrelated.wait().expect("reap unrelated");
}

const LATEST_MODE: &str = "CCCC_GUARD_LATEST_TEST";

#[test]
fn a_group_published_while_the_watchdog_is_paused_still_ends_with_its_owner() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut owner = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "process_tree::guard::tests::latest_list_owner_helper",
            "--exact",
            "--nocapture",
        ])
        .env(LATEST_MODE, "1")
        .env(STALL_DIR, temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn owner helper");
    let pids_path = temp.path().join("pids");
    let deadline = Instant::now() + Duration::from_secs(10);
    let pids = loop {
        if let Ok(text) = std::fs::read_to_string(&pids_path)
            && text.ends_with('\n')
        {
            break text
                .split_whitespace()
                .map(|pid| pid.parse::<i32>().expect("pid"))
                .collect::<Vec<_>>();
        }
        assert!(Instant::now() < deadline, "owner helper did not publish");
        std::thread::sleep(Duration::from_millis(20));
    };
    let (first, latest) = (pids[0], pids[1]);

    owner.kill().expect("kill owner");
    owner.wait().expect("reap owner");

    // The watchdog must act now, without a later owner start.
    let first_ended = exited_within(first, Duration::from_secs(10));
    let latest_ended = exited_within(latest, Duration::from_secs(10));
    for pid in [first, latest] {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    assert!(first_ended, "an earlier group survived its owner");
    assert!(
        latest_ended,
        "the group published while the watchdog was paused survived"
    );
}

#[test]
fn latest_list_owner_helper() {
    if std::env::var_os(LATEST_MODE).is_none() {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os(STALL_DIR).expect("dir"));
    crate::protect_owned_process_groups(&dir.join("ledger.json")).expect("protect");
    let spawn = || {
        crate::OwnedProcessTree::spawn(
            Command::new("sleep")
                .arg("300")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .expect("spawn owned group")
    };
    let (first, _first_tree) = spawn();
    let output = Command::new("pgrep")
        .args(["-P", &std::process::id().to_string(), "-x", "sh"])
        .output()
        .expect("pgrep");
    let watchdog = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(
        Command::new("kill")
            .args(["-STOP", &watchdog])
            .status()
            .expect("stop watchdog")
            .success()
    );
    let (latest, _latest_tree) = spawn();
    std::fs::write(
        dir.join("pids"),
        format!("{} {}\n", first.id(), latest.id()),
    )
    .expect("publish pids");
    std::thread::sleep(Duration::from_secs(300));
}

const INSTANCE_MODE: &str = "CCCC_GUARD_INSTANCE_TEST";
const INSTANCE_OUT: &str = "CCCC_GUARD_INSTANCE_OUT";

#[test]
fn a_late_watchdog_of_a_replaced_instance_never_touches_the_new_instance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut first = spawn_instance(temp.path(), "first");
    let (first_group, first_list) = instance_output(temp.path(), "first");
    first.kill().expect("kill first instance");
    first.wait().expect("reap first instance");
    assert!(
        ended(first_group),
        "the first instance's watchdog left its group"
    );

    let second = KillOnDrop(spawn_instance(temp.path(), "second"));
    let (second_group, second_list) = instance_output(temp.path(), "second");
    assert_ne!(first_list, second_list);
    assert!(
        !first_list.exists(),
        "the replaced instance's list must be removed"
    );

    // The first instance's watchdog reads its list only now.
    run_watchdog(&first_list);

    let survived = running(second_group);
    drop(second);
    // Its own watchdog reads the list before the directory goes away.
    let second_ended = ended(second_group);
    assert!(
        survived,
        "a late watchdog terminated the new instance's group"
    );
    assert!(
        second_ended,
        "the second instance's watchdog left its group"
    );
}

fn ended(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while running(pid) {
        if Instant::now() >= deadline {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

#[test]
fn instance_helper() {
    let Some(name) = std::env::var_os(INSTANCE_MODE) else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var_os(INSTANCE_OUT).expect("dir"));
    crate::protect_owned_process_groups(&dir.join("owned.json")).expect("protect");
    let (group, _tree) = crate::OwnedProcessTree::spawn(
        Command::new("sleep")
            .arg("300")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .expect("spawn owned group");
    let list = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "watchdog"))
        .expect("own watch list");
    std::fs::write(
        dir.join(name).with_extension("out"),
        format!("{} {}\n", group.id(), list.display()),
    )
    .expect("publish instance");
    std::thread::sleep(Duration::from_secs(300));
}

fn spawn_instance(dir: &Path, name: &str) -> Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "process_tree::guard::tests::instance_helper",
            "--exact",
            "--nocapture",
        ])
        .env(INSTANCE_MODE, name)
        .env(INSTANCE_OUT, dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn instance helper")
}

fn instance_output(dir: &Path, name: &str) -> (i32, std::path::PathBuf) {
    let path = dir.join(name).with_extension("out");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Some((group, list)) = text.trim_end().split_once(' ')
            && text.ends_with('\n')
        {
            return (group.parse().expect("group pid"), list.into());
        }
        assert!(Instant::now() < deadline, "{name} instance did not publish");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A terminated child of a still-running instance stays a zombie; only a process
/// that can still run counts.
fn running(pid: i32) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let state = String::from_utf8_lossy(&output.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

/// Stops an instance helper even when an assertion fails first.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
