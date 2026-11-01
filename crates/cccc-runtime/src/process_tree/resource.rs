use std::io;

pub(super) struct Resource {
    #[cfg(unix)]
    pid: rustix::process::Pid,
    #[cfg(unix)]
    spawned_at: u64,
    #[cfg(unix)]
    record_id: uuid::Uuid,
    #[cfg(unix)]
    origin: Option<super::guard_record::Origin>,
    #[cfg(unix)]
    evidence: super::guard_record::GroupEvidence,
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
        Ok(Self {
            pid,
            spawned_at: super::guard::now(),
            record_id: uuid::Uuid::new_v4(),
            origin: None,
            evidence: super::guard_record::GroupEvidence::Unconfirmed,
        })
    }

    #[cfg(unix)]
    pub(super) fn attach_intent(&mut self, intent: Option<&super::guard::PreparedIntent>) {
        let Some(intent) = intent else { return };
        self.record_id = intent.entry.id;
        self.origin = intent.entry.origin.clone();
        #[cfg(target_os = "linux")]
        if intent.may_capture_identity
            && let Ok(view) = super::linux_identity::ProcView::open()
            && let Ok(captured) = view.capture_child(self.pid.as_raw_pid())
            && self.origin.as_ref() == Some(&captured.origin)
        {
            self.evidence = super::guard_record::GroupEvidence::Captured(captured);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = intent.may_capture_identity;
    }

    /// Every Unix resource leads its own process group.
    #[cfg(unix)]
    pub(super) fn owned_group(&self) -> super::guard::OwnedGroup {
        super::guard::OwnedGroup {
            pgid: self.pid.as_raw_pid(),
            spawned_at: self.spawned_at,
            id: self.record_id,
            origin: self.origin.clone(),
            evidence: self.evidence.clone(),
        }
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
        // A reaped child no longer reserves its numeric process group. Refuse
        // to signal when a competing waiter or auto-reaper removed ownership.
        let _ = self.exited()?;
        match killpg(Pid::from_raw(self.pid.as_raw_pid()), signal) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            // Darwin reports EPERM for an unreaped, exited group with no
            // signalable members. A live leader must still surface EPERM.
            Err(Errno::EPERM)
                if cfg!(target_vendor = "apple") && self.exited_after_signal_race()? =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    #[cfg(unix)]
    fn exited_after_signal_race(&self) -> io::Result<bool> {
        // Darwin can stop accepting signals just before waitid publishes the exit.
        // Keep the leader unreaped while observing that short transition. A live
        // leader still returns EPERM; this never turns a permission denial into
        // successful cleanup merely because time elapsed.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(20);
        loop {
            if self.exited()? {
                return Ok(true);
            }
            if std::time::Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
