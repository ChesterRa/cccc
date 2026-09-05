use std::io;

pub(super) struct Resource {
    #[cfg(unix)]
    pid: rustix::process::Pid,
    #[cfg(windows)]
    job: Option<win32job::Job>,
}

impl Resource {
    pub(super) fn standard(child: &std::process::Child) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Self::unix(child.id())
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            Self::windows(child.as_raw_handle() as isize)
        }
    }

    pub(super) fn pty(child: &dyn portable_pty::Child) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Self::unix(
                child
                    .process_id()
                    .ok_or_else(|| io::Error::other("PTY child has no PID"))?,
            )
        }
        #[cfg(windows)]
        {
            Self::windows(
                child
                    .as_raw_handle()
                    .ok_or_else(|| io::Error::other("PTY child has no handle"))?
                    as isize,
            )
        }
    }

    #[cfg(unix)]
    fn unix(pid: u32) -> io::Result<Self> {
        let pid = rustix::process::Pid::from_raw(pid as i32)
            .ok_or_else(|| io::Error::other("invalid child PID"))?;
        Ok(Self { pid })
    }

    #[cfg(windows)]
    fn windows(handle: isize) -> io::Result<Self> {
        let mut limits = win32job::ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        let job = win32job::Job::create_with_limit_info(&limits)
            .map_err(|e| io::Error::other(e.to_string()))?;
        job.assign_process(handle)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { job: Some(job) })
    }

    #[cfg(unix)]
    pub(super) fn exited(&self) -> io::Result<bool> {
        use rustix::process::{WaitId, WaitIdOptions, waitid};
        // WNOWAIT retains the zombie leader, preventing PID/PGID reuse until
        // we have killed remaining group members and revoked registration.
        waitid(
            WaitId::Pid(self.pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map(|status| status.is_some())
        .map_err(Into::into)
    }

    pub(super) fn terminate(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.signal(nix::sys::signal::Signal::SIGKILL)
        }
        #[cfg(windows)]
        {
            self.job.take();
            Ok(())
        }
    }

    pub(super) fn request_stop(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.signal(nix::sys::signal::Signal::SIGTERM)
        }
        #[cfg(windows)]
        {
            self.terminate()
        }
    }

    #[cfg(unix)]
    fn signal(&self, signal: nix::sys::signal::Signal) -> io::Result<()> {
        use nix::{errno::Errno, sys::signal::killpg, unistd::Pid};
        match killpg(Pid::from_raw(self.pid.as_raw_pid()), signal) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            // Darwin reports EPERM for an unreaped, exited group with no
            // signalable members. A live leader must still surface EPERM.
            Err(Errno::EPERM) if cfg!(target_vendor = "apple") && self.exited()? => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
