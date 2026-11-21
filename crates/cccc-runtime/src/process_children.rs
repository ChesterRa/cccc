//! The direct children of a process, as the operating system lists them now.

use std::io;

/// PIDs whose parent is `parent`, exited but unreaped children included. The
/// list is a snapshot: a child started while it is read may be missing.
pub fn children_of(parent: u32) -> io::Result<Vec<u32>> {
    #[cfg(target_os = "linux")]
    return linux_children(parent);
    #[cfg(target_os = "macos")]
    return libproc::processes::pids_by_type(libproc::processes::ProcFilter::ByParentProcess {
        ppid: parent,
    });
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = parent;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "listing child processes is not implemented on this platform",
        ))
    }
}

#[cfg(target_os = "linux")]
fn linux_children(parent: u32) -> io::Result<Vec<u32>> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir("/proc")?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // The command name may contain spaces or parentheses; fields resume
        // after its last ')': state, then the parent PID.
        let ppid = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|field| field.parse::<u32>().ok());
        if ppid == Some(parent) {
            children.push(pid);
        }
    }
    Ok(children)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// Whether `pid` has exited and waits for its parent; reading reaps nothing.
    #[cfg(target_os = "linux")]
    fn exited_unreaped(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                == Some("Z")
        })
    }

    /// Whether `pid` has exited and waits for its parent; reading reaps nothing.
    #[cfg(target_os = "macos")]
    fn exited_unreaped(pid: u32) -> bool {
        use libproc::bsd_info::BSDInfo;
        // `SZOMB` in <sys/proc.h>; a nonzero argument lets the kernel report zombies.
        const SZOMB: u32 = 5;
        i32::try_from(pid).is_ok_and(|pid| {
            libproc::proc_pid::pidinfo::<BSDInfo>(pid, 1).is_ok_and(|info| info.pbi_status == SZOMB)
        })
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn lists_a_running_and_an_exited_unreaped_child_but_not_the_caller() {
        let mut running = Command::new("sleep").arg("30").spawn().expect("child");
        let mut exited = Command::new("true").spawn().expect("short child");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !exited_unreaped(exited.id()) {
            assert!(Instant::now() < deadline, "the short child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        let me = std::process::id();
        let children = super::children_of(me).expect("children");
        assert!(children.contains(&running.id()), "{children:?}");
        assert!(children.contains(&exited.id()), "{children:?}");
        assert!(!children.contains(&me));
        running.kill().expect("stop child");
        running.wait().expect("reap child");
        exited.wait().expect("reap short child");
    }
}
