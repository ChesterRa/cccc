use super::CommandError;
use super::resource::Resource;
#[cfg(unix)]
use std::io;
use std::process::{Child, Command};

#[cfg(unix)]
pub(super) fn standard(
    command: &mut Command,
    _creation_flags: u32,
) -> Result<(Child, Resource), CommandError> {
    standard_with_resource(command, |child| Resource::standard(child))
}

#[cfg(unix)]
fn standard_with_resource(
    command: &mut Command,
    resource: impl FnOnce(&mut Child) -> io::Result<Resource>,
) -> Result<(Child, Resource), CommandError> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command.spawn().map_err(CommandError::NotStarted)?;
    match resource(&mut child) {
        Ok(resource) => Ok((child, resource)),
        Err(error) => {
            cleanup_unregistered_child(&mut child);
            Err(CommandError::Started(super::unconfirmed_start(error)))
        }
    }
}

#[cfg(unix)]
fn cleanup_unregistered_child(child: &mut Child) {
    use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, waitid};
    let Some(pid) = Pid::from_raw(child.id() as i32) else {
        return;
    };
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    // The group number is safe only while this process is still our unreaped
    // child. An external reaper invalidates that assumption.
    if waitid(WaitId::Pid(pid), options).is_err() {
        return;
    }
    // getpgrp can be 0 when the caller's group is outside its PID namespace.
    if nix::unistd::getpgrp().as_raw() != pid.as_raw_nonzero().get() {
        let _ = rustix::process::kill_process_group(pid, Signal::KILL);
    }
    stop_owned_child(child);
}

/// Stop and reap the still-owned leader separately from its original group.
/// A standard child can have moved to another group before registration.
#[cfg(unix)]
pub(super) fn stop_owned_child(child: &mut Child) {
    use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
    let Some(pid) = Pid::from_raw(child.id() as i32) else {
        return;
    };
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    if waitid(WaitId::Pid(pid), options).is_err() {
        return;
    }
    // The leader may already have moved away from its original group. Do not
    // wait on a still-running child if signalling it failed.
    if child.kill().is_ok()
        || waitid(WaitId::Pid(pid), options).is_ok_and(|status| status.is_some())
    {
        let _ = child.wait();
    }
}

#[cfg(windows)]
pub(super) fn standard(
    command: &mut Command,
    creation_flags: u32,
) -> Result<(Child, Resource), CommandError> {
    let mut created = false;
    let suspended = cccc_windows_process::spawn_suspended_with_creation_observer(
        command,
        creation_flags,
        || created = true,
    )
    .map_err(|error| {
        if created {
            CommandError::Started(error)
        } else {
            CommandError::NotStarted(error)
        }
    })?;
    let resource = Resource::standard(suspended.child()).map_err(CommandError::Started)?;
    // The Job owns the child before any child code can create descendants.
    // Either error path drops the Job and the suspended-child cleanup guard.
    let child = suspended.resume().map_err(CommandError::Started)?;
    Ok((child, resource))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::process::Stdio;

    #[cfg(target_os = "linux")]
    #[test]
    fn an_inherited_group_outside_the_pid_namespace_does_not_panic() {
        use std::os::unix::process::CommandExt;
        const MODE: &str = "CCCC_TEST_OUTSIDE_PIDNS_GROUP";
        if let Ok(mode) = std::env::var(MODE) {
            assert_eq!(nix::unistd::getpgrp().as_raw(), 0);
            if mode == "recover" {
                use super::super::guard_record::{GroupEvidence, GroupRecord};
                let view = super::super::linux_identity::ProcView::open().expect("procfs");
                let unconfirmed = GroupRecord {
                    id: uuid::Uuid::new_v4(),
                    source_id: None,
                    pgid: 2,
                    spawned_at: 1,
                    observed_at: 1,
                    origin: None,
                    evidence: GroupEvidence::Unconfirmed,
                };
                assert!(
                    view.signal_group(&unconfirmed, rustix::process::Signal::TERM)
                        .is_err()
                );
            } else {
                let mut child = Command::new("/bin/sleep")
                    .arg("30")
                    .process_group(0)
                    .spawn()
                    .expect("owned child");
                cleanup_unregistered_child(&mut child);
                assert!(child.try_wait().expect("poll child").is_some());
            }
            return;
        }

        let executable = std::env::current_exe().expect("test executable");
        let name = "process_tree::spawn::tests::an_inherited_group_outside_the_pid_namespace_does_not_panic";
        for mode in ["recover", "spawn"] {
            let output = Command::new("unshare")
                .args([
                    "--map-current-user",
                    "--mount",
                    "--pid",
                    "--fork",
                    "--mount-proc",
                ])
                .arg(&executable)
                .args(["--exact", name, "--nocapture"])
                .env(MODE, mode)
                .output();
            let output = match output {
                Ok(output) => output,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    eprintln!("skipping: unshare is unavailable");
                    return;
                }
                Err(error) => panic!("start namespace fixture: {error}"),
            };
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() && stderr.starts_with("unshare:") {
                eprintln!("skipping: cannot create a private PID namespace");
                return;
            }
            assert!(
                output.status.success(),
                "{mode} fixture failed: {} {stderr}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }

    #[test]
    fn registration_failure_after_child_execution_is_not_a_spawn_failure() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf started; exec sleep 60"])
            .stdout(Stdio::piped());
        let mut pid = None;
        let result = standard_with_resource(&mut command, |child| {
            pid = Some(child.id());
            // Observe real child code, then fail at the resource boundary with
            // the SAME error kind as an absent executable. Kind is not stage.
            let mut stdout = BufReader::new(child.stdout.as_mut().expect("stdout"));
            let mut started = [0; 7];
            std::io::Read::read_exact(&mut stdout, &mut started).expect("child ran");
            assert_eq!(&started, b"started");
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "injected registration failure",
            ))
        });
        let Err(CommandError::Started(error)) = result else {
            panic!("a child that executed must not be labelled NotStarted");
        };
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(
            error
                .get_ref()
                .is_some_and(|source| source.is::<crate::ProcessStartUnconfirmed>())
        );
        assert_eq!(
            nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(pid.expect("spawned PID") as i32),
                None
            ),
            Err(nix::errno::Errno::ECHILD),
            "failed registration still reaps the owned child"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registration_failure_stops_the_owned_group_as_well_as_the_leader() {
        use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};
        use std::io::BufRead;
        struct PinnedFixture(std::os::fd::OwnedFd, i32);
        impl Drop for PinnedFixture {
            fn drop(&mut self) {
                let _ = pidfd_send_signal(&self.0, Signal::KILL);
            }
        }

        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 60 & echo $!; wait"])
            .stdout(Stdio::piped());
        let mut descendant = None;
        let result = standard_with_resource(&mut command, |child| {
            let mut stdout = BufReader::new(child.stdout.as_mut().expect("stdout"));
            let mut line = String::new();
            stdout.read_line(&mut line).expect("descendant pid");
            let pid =
                Pid::from_raw(line.trim().parse().expect("numeric pid")).expect("positive pid");
            descendant = Some(PinnedFixture(
                pidfd_open(pid, PidfdFlags::empty()).expect("fixture pidfd"),
                pid.as_raw_pid(),
            ));
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "injected registration failure",
            ))
        });
        assert!(matches!(result, Err(CommandError::Started(_))));
        let fixture = descendant.expect("descendant created");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match std::fs::read_to_string(format!("/proc/{}/stat", fixture.1)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Ok(stat)
                    if stat
                        .rsplit_once(')')
                        .is_some_and(|(_, tail)| tail.trim_start().starts_with('Z')) =>
                {
                    break;
                }
                Ok(_) => {}
                Err(error) => panic!("could not inspect fixture: {error}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "owned descendant survived registration failure"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
