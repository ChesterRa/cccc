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
            let _ = child.kill();
            let _ = child.wait();
            Err(CommandError::Started(error))
        }
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
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(
            nix::sys::wait::waitpid(
                nix::unistd::Pid::from_raw(pid.expect("spawned PID") as i32),
                None
            ),
            Err(nix::errno::Errno::ECHILD),
            "failed registration still reaps the owned child"
        );
    }
}
