//! Whether an installer may replace the program while no daemon runs. The
//! check starts, registers and reconciles nothing, creates or changes no file
//! under CCCC_HOME, and the locks it takes end with the check.

use super::{client, receipt, transport::Paths};
use cccc_core::HomeLayout;
use std::{
    fs::OpenOptions,
    io,
    path::Path,
    time::{Duration, Instant},
};

/// Time for every kept holder together to answer Hello.
const HELLO_BUDGET: Duration = Duration::from_secs(5);

/// Ok allows the replacement; Err is the reason, with the next step.
pub(super) async fn check(home: &HomeLayout) -> Result<(), String> {
    check_within(home, HELLO_BUDGET).await
}

async fn check_within(home: &HomeLayout, budget: Duration) -> Result<(), String> {
    // A daemon takes this lock before it answers, so a daemon that is still
    // starting holds it. Held until the check returns, so none starts during it.
    let lock_path = home.daemon_dir().join("ccccd.lock");
    let _home_lock = match lock_existing(&lock_path) {
        Ok(Lock::Missing) => None,
        Ok(Lock::Held(lock)) => Some(lock),
        Ok(Lock::Busy) => {
            return Err(format!(
                "another CCCC process holds {}, so CCCC may be starting or stopping; wait until it has finished",
                lock_path.display()
            ));
        }
        Err(error) => return Err(unreadable(&lock_path, &error)),
    };
    let directory = home.daemon_dir().join("session-owners");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        // No holder was ever started here, so there is nothing to take over.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(unreadable(&directory, &error)),
    };
    let mut live = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| unreadable(&directory, &error))?;
        let is_directory = entry
            .file_type()
            .map_err(|error| unreadable(&entry.path(), &error))?
            .is_dir();
        if !is_directory {
            continue;
        }
        let paths = Paths::new(home, &entry.file_name().to_string_lossy())
            .map_err(|error| unreadable(&entry.path(), &error))?;
        // A free lock means the holder exited; the next daemon start reviews
        // whatever it left before restarting any member.
        match lock_existing(&paths.lock).map_err(|error| unreadable(&paths.lock, &error))? {
            Lock::Busy => live.push(paths.owner_id),
            Lock::Missing | Lock::Held(_) => {}
        }
    }
    if live.is_empty() {
        return Ok(());
    }
    live.sort();
    let kept = receipt::last_exit_kept_agents(home)
        .map_err(|error| unreadable(&home.daemon_dir().join("shutdown-result.json"), &error))?;
    if !kept {
        return Err(format!(
            "CCCC agents may still be running without a daemon ({}), and CCCC did not keep them for an update; stop them with 'cccc daemon stop' from the CCCC installation that last ran the daemon, following what it reports",
            holders(&live)
        ));
    }
    let deadline = Instant::now() + budget;
    for owner_id in &live {
        let refusal = match client::load(home, owner_id) {
            Ok(address) => client::answers(&address, deadline).await.err(),
            Err(error) => Some(io::Error::new(
                error.kind(),
                format!(
                    "its address could not be read ({error}); stop its agents with 'cccc daemon stop' from the CCCC installation that last ran the daemon, following what it reports"
                ),
            )),
        };
        if let Some(error) = refusal {
            return Err(format!(
                "CCCC kept its agents for an update, but this version cannot take over session owner {owner_id}: {error}"
            ));
        }
    }
    Ok(())
}

enum Lock {
    Missing,
    Busy,
    Held(std::fs::File),
}

/// Try a lock file that its process creates, without creating it here.
fn lock_existing(path: &Path) -> io::Result<Lock> {
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Lock::Missing),
        Err(error) => return Err(error),
    };
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Lock::Held(file)),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Ok(Lock::Busy)
        }
        Err(error) => Err(error),
    }
}

fn holders(owner_ids: &[String]) -> String {
    match owner_ids {
        [owner_id] => format!("session owner {owner_id}"),
        _ => format!("session owners {}", owner_ids.join(", ")),
    }
}

fn unreadable(path: &Path, error: &io::Error) -> String {
    format!(
        "CCCC could not check {} ({error}), so it cannot confirm that no agent is left without a daemon",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_owner::receipt::{ShutdownMode, ShutdownReport, write_shutdown_report};

    fn home() -> (tempfile::TempDir, HomeLayout) {
        let directory = tempfile::tempdir().expect("private test directory");
        let home = HomeLayout::from_path(directory.path().join("home")).expect("home path");
        (directory, home)
    }

    fn check_now(home: &HomeLayout) -> Result<(), String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(check(home))
    }

    fn owner(home: &HomeLayout) -> Paths {
        let paths = Paths::new(home, &uuid::Uuid::new_v4().simple().to_string()).expect("paths");
        paths.initialize().expect("owner directory");
        paths
    }

    fn hold(lock: &Path) -> std::fs::File {
        cccc_core::fs::try_exclusive_lock(lock)
            .expect("lock")
            .expect("held")
    }

    fn receipt(home: &HomeLayout, mode: ShutdownMode) {
        write_shutdown_report(
            home,
            &ShutdownReport {
                v: 1,
                daemon_pid: 4242,
                request_id: "request".into(),
                mode,
                confirmed_stopped: Vec::new(),
                retained: Vec::new(),
                unconfirmed: Vec::new(),
            },
        )
        .expect("receipt");
    }

    #[test]
    fn a_home_without_holders_is_left_untouched() {
        let (_directory, home) = home();
        assert_eq!(check_now(&home), Ok(()));
        assert!(!home.root().exists());
        std::fs::create_dir_all(home.daemon_dir()).expect("daemon directory");
        assert_eq!(check_now(&home), Ok(()));
        assert!(!home.daemon_dir().join("ccccd.lock").exists());
    }

    #[test]
    fn exited_holders_leave_the_decision_to_the_next_start_and_gain_no_files() {
        let (_directory, home) = home();
        let exited = owner(&home);
        assert_eq!(check_now(&home), Ok(()));
        assert!(!exited.lock.exists());
        assert!(!home.daemon_dir().join("ccccd.lock").exists());
        drop(hold(&exited.lock));
        assert_eq!(check_now(&home), Ok(()));
        let names = std::fs::read_dir(home.daemon_dir().join("session-owners"))
            .expect("owner directories")
            .count();
        assert_eq!(names, 1);
    }

    #[test]
    fn a_running_or_starting_daemon_refuses_before_it_has_any_holder() {
        let (_directory, home) = home();
        let daemon = hold(&home.daemon_dir().join("ccccd.lock"));
        let refusal = check_now(&home).expect_err("daemon holds the home");
        assert!(refusal.contains("starting or stopping"), "{refusal}");
        owner(&home);
        let refusal = check_now(&home).expect_err("daemon holds the home");
        assert!(refusal.contains("starting or stopping"), "{refusal}");
        drop(daemon);
        assert_eq!(check_now(&home), Ok(()));
    }

    #[test]
    fn running_holders_need_a_kept_exit() {
        let (_directory, home) = home();
        let running = owner(&home);
        let _holder = hold(&running.lock);
        for mode in [None, Some(ShutdownMode::Stop)] {
            if let Some(mode) = mode {
                receipt(&home, mode);
            }
            let refusal = check_now(&home).expect_err("not kept for an update");
            assert!(refusal.contains("did not keep them"), "{refusal}");
            assert!(refusal.contains(&running.owner_id), "{refusal}");
        }
        std::fs::write(home.daemon_dir().join("shutdown-result.json"), b"{")
            .expect("damaged receipt");
        let refusal = check_now(&home).expect_err("unreadable receipt");
        assert!(refusal.contains("could not check"), "{refusal}");
    }

    #[test]
    fn a_kept_holder_that_cannot_be_reached_refuses() {
        let (_directory, home) = home();
        let running = owner(&home);
        let _holder = hold(&running.lock);
        receipt(&home, ShutdownMode::PreserveAgents);
        let refusal = check_now(&home).expect_err("no address to reach");
        assert!(refusal.contains("cannot take over"), "{refusal}");
        assert!(refusal.contains(&running.owner_id), "{refusal}");
    }

    /// A release with another owner contract cannot take over or stop this
    /// holder, so the installation that started it has to.
    #[test]
    fn a_kept_holder_of_another_contract_is_left_to_its_own_installation() {
        let (_directory, home) = home();
        let running = owner(&home);
        let _holder = hold(&running.lock);
        cccc_core::fs::write_json(
            &running.address,
            &super::super::transport::Address {
                owner_id: running.owner_id.clone(),
                instance: "other".into(),
                contract: super::super::protocol::CONTRACT + 1,
                pid: std::process::id(),
                socket: running.socket.clone(),
            },
        )
        .expect("address");
        receipt(&home, ShutdownMode::PreserveAgents);
        let refusal = check_now(&home).expect_err("another contract");
        assert!(refusal.contains("cannot take over"), "{refusal}");
        assert!(refusal.contains(&running.owner_id), "{refusal}");
        assert!(
            refusal
                .contains("'cccc daemon stop' from the CCCC installation that last ran the daemon"),
            "{refusal}"
        );
    }

    #[test]
    fn an_ordinary_offline_stop_withdraws_a_keep_even_when_it_fails() {
        let (_directory, home) = home();
        let running = owner(&home);
        let _holder = hold(&running.lock);
        receipt(&home, ShutdownMode::PreserveAgents);
        assert!(super::super::controller::stop_offline(&home).is_err());
        assert!(!home.daemon_dir().join("shutdown-result.json").exists());
        let refusal = check_now(&home).expect_err("the Keep was withdrawn");
        assert!(refusal.contains("did not keep them"), "{refusal}");
    }

    #[test]
    fn a_kept_holder_that_accepts_but_never_answers_refuses_within_the_budget() {
        let (_directory, home) = home();
        let running = owner(&home);
        let _holder = hold(&running.lock);
        let _listener = {
            let socket = cccc_core::unix_socket_path::SocketPath::new(&running.socket)
                .expect("short socket alias");
            std::os::unix::net::UnixListener::bind(&socket.path).expect("listen without reply")
        };
        cccc_core::fs::write_json(
            &running.address,
            &super::super::transport::Address {
                owner_id: running.owner_id.clone(),
                instance: "silent".into(),
                contract: super::super::protocol::CONTRACT,
                pid: std::process::id(),
                socket: running.socket.clone(),
            },
        )
        .expect("address");
        receipt(&home, ShutdownMode::PreserveAgents);
        let started = Instant::now();
        let refusal = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(check_within(&home, Duration::from_millis(300)))
            .expect_err("no Hello reply");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(refusal.contains("cannot take over"), "{refusal}");
        assert!(refusal.contains("TimedOut"), "{refusal}");
    }

    /// A real holder: one that answers is taken over, and the same holder
    /// stopped by a signal is refused within the budget.
    #[cfg(all(
        debug_assertions,
        any(target_os = "linux", all(target_os = "macos", target_arch = "aarch64"))
    ))]
    #[test]
    fn a_kept_holder_must_answer_now() {
        use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process, waitid};
        let (_directory, home) = home();
        home.initialize().expect("initialize home");
        let owner_id = uuid::Uuid::new_v4().simple().to_string();
        let started = {
            let _daemon = hold(&home.daemon_dir().join("ccccd.lock"));
            client::start(&home, &owner_id).expect("start a holder")
        };
        let pid = Pid::from_raw(
            i32::try_from(client::load(&home, &owner_id).expect("address").pid).expect("pid"),
        )
        .expect("positive owner PID");
        receipt(&home, ShutdownMode::PreserveAgents);
        let answered = check_now(&home);
        kill_process(pid, Signal::STOP).expect("signal the holder");
        // Sending SIGSTOP does not wait for the child to stop. Observe its
        // state without reaping it; the launching client already has a waiter.
        let state = rustix::io::retry_on_intr(|| {
            waitid(
                WaitId::Pid(pid),
                WaitIdOptions::STOPPED | WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
            )
        });
        let stopped = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(check_within(&home, Duration::from_millis(500)));
        kill_process(pid, Signal::CONT).expect("continue the holder");
        started.retire().expect("retire the empty holder");
        assert_eq!(answered, Ok(()));
        assert!(
            state.expect("observe holder").is_some_and(|s| s.stopped()),
            "the kernel did not report the holder stopped"
        );
        let refusal = stopped.expect_err("a stopped holder does not answer");
        assert!(refusal.contains(&owner_id), "{refusal}");
        assert!(refusal.contains("cannot take over"), "{refusal}");
        assert!(refusal.contains("TimedOut"), "{refusal}");
    }
}
