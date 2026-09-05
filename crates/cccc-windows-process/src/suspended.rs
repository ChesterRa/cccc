use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, GetProcessIdOfThread, OpenThread, ResumeThread,
    THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

/// A child whose primary thread has never executed. Dropping without `resume`
/// terminates and reaps it, including Job-assignment and resume failure paths.
pub struct SuspendedChild {
    child: Option<Child>,
    thread: Option<OwnedHandle>,
}

/// `creation_flags` is the caller's complete Windows creation-flag set; it is
/// combined with CREATE_SUSPENDED, not silently replaced by that flag alone.
pub fn spawn_suspended(command: &mut Command, creation_flags: u32) -> io::Result<SuspendedChild> {
    command.creation_flags(creation_flags | CREATE_SUSPENDED);
    let result = command.spawn();
    // Command remains reusable with the caller's original flags.
    command.creation_flags(creation_flags);
    let mut suspended = SuspendedChild {
        child: Some(result?),
        thread: None,
    };
    suspended.thread = Some(primary_thread(suspended.child().id())?);
    Ok(suspended)
}

impl SuspendedChild {
    pub fn child(&self) -> &Child {
        self.child
            .as_ref()
            .expect("suspended child is owned until resume")
    }

    pub fn resume(mut self) -> io::Result<Child> {
        let thread = self
            .thread
            .as_ref()
            .expect("spawn retained the primary thread");
        // SAFETY: thread is an owned handle with THREAD_SUSPEND_RESUME rights,
        // retained while the corresponding child process is still owned.
        let previous = unsafe { ResumeThread(thread.as_raw_handle()) };
        if previous == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        if previous != 1 {
            return Err(io::Error::other("unexpected primary-thread suspend count"));
        }
        Ok(self.child.take().expect("resume consumes the child once"))
    }
}

impl Drop for SuspendedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn primary_thread(pid: u32) -> io::Result<OwnedHandle> {
    // std::process::Command closes CreateProcess's thread handle. The primary
    // thread is the child's only thread before its loader/user code is resumed.
    // Recover that handle via the documented ToolHelp API; no NT-only APIs.
    // SAFETY: scalar flags/PID, with no caller pointers.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: checked successful owned snapshot handle; transferred exactly once.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    // SAFETY: THREADENTRY32 consists only of integer fields; zero is valid.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    // SAFETY: snapshot remains alive and entry is correctly sized writable storage.
    let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while found != 0 {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: scalar access rights/thread ID; the returned handle is checked.
            let thread = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    entry.th32ThreadID,
                )
            };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: successful owned thread handle, transferred exactly once.
            let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
            // SAFETY: owned thread handle has query rights. Verify identity in
            // case the thread changed between the snapshot and OpenThread.
            if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
                return Err(io::Error::other("primary thread identity changed"));
            }
            return Ok(thread);
        }
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        // SAFETY: same valid snapshot and writable entry as Thread32First.
        found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    Err(io::Error::other(
        "suspended child's primary thread is unavailable",
    ))
}

#[cfg(test)]
impl SuspendedChild {
    pub(crate) fn take_stdout_for_test(&mut self) -> Option<std::process::ChildStdout> {
        self.child.as_mut()?.stdout.take()
    }
}
