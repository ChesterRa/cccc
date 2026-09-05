use super::resource::Resource;
use std::io;
use std::process::{Child, Command};

#[cfg(unix)]
pub(super) fn standard(
    command: &mut Command,
    _creation_flags: u32,
) -> io::Result<(Child, Resource)> {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command.spawn()?;
    match Resource::standard(&child) {
        Ok(resource) => Ok((child, resource)),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

#[cfg(windows)]
pub(super) fn standard(
    command: &mut Command,
    creation_flags: u32,
) -> io::Result<(Child, Resource)> {
    let suspended = cccc_windows_process::spawn_suspended(command, creation_flags)?;
    let resource = Resource::standard(suspended.child())?;
    // The Job owns the child before any child code can create descendants.
    // Either error path drops the Job and the suspended-child cleanup guard.
    let child = suspended.resume()?;
    Ok((child, resource))
}
