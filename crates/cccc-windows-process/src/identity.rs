//! Read-only native PID identity. Creation FILETIME is absolute (100 ns), so
//! unlike an uptime tick it does not repeat at the next boot.
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::Foundation::{
    ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, FILETIME, INVALID_HANDLE_VALUE, WAIT_FAILED,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
};

pub fn creation_time(pid: u32) -> io::Result<u64> {
    let process = open(pid)?;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: retained query handle and four distinct writable FILETIME values.
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: retained handle has SYNCHRONIZE rights; zero timeout never waits.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
        WAIT_TIMEOUT => {
            Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        }
        WAIT_OBJECT_0 => Err(missing()),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other("Native process state unavailable")),
    }
}

pub fn image_path(pid: u32) -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let process = open(pid)?;
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    // SAFETY: retained query handle; length describes the writable UTF-16 buffer.
    if unsafe {
        QueryFullProcessImageNameW(
            process.as_raw_handle(),
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(std::ffi::OsString::from_wide(&buffer[..length as usize]).into())
}

/// The caller must also compare child/parent creation identities: ToolHelp's
/// recorded parent PID alone does not prevent reuse after that parent exits.
pub fn parent_pid(pid: u32) -> io::Result<u32> {
    // SAFETY: scalar snapshot flags; no caller memory is referenced.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: checked successful owned snapshot, transferred exactly once.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: valid retained snapshot and correctly sized writable entry.
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
    while found != 0 {
        if entry.th32ProcessID == pid {
            return Ok(entry.th32ParentProcessID);
        }
        // SAFETY: same valid snapshot and writable entry as Process32FirstW.
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
    }
    let error = io::Error::last_os_error();
    Err(
        if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
            missing()
        } else {
            error
        },
    )
}

fn open(pid: u32) -> io::Result<OwnedHandle> {
    // SAFETY: scalar PID and access flags; no inherited handle or caller memory.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if process.is_null() {
        let error = io::Error::last_os_error();
        return Err(
            if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                missing()
            } else {
                error
            },
        );
    }
    // SAFETY: successful owned process handle, transferred exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(process) })
}

fn missing() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "Native host has exited")
}
